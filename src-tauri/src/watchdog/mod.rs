//! Native recovery helper. The helper owns every write in a diagnostic session.
pub(crate) mod client;
pub(crate) mod process;
pub mod protocol;
pub(crate) mod session;
#[cfg(debug_assertions)]
pub mod simulation;

use crate::{
    color_engine::magnification::{MagnificationApi, NativeMagnification},
    single_instance::InstanceGuard,
};
use protocol::{Command, Event, MAX_MS, VERSION};
use std::{
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Incident lockdown: real color setters stay disabled in both shipped processes.
/// Debug simulations keep their fake API available for tests only.
pub const REAL_DIAGNOSTIC_ENABLED: bool = false;

pub(crate) const fn apply_allowed(simulated: bool) -> bool {
    simulated || REAL_DIAGNOSTIC_ENABLED
}
pub const RECOVERY_BLOCKER: &str = "El watchdog cuenta con preflight de solo lectura, pero no se han validado escrituras ni restauración real de Magnification. El diagnóstico visual y la activación normal siguen bloqueados hasta completar una prueba manual controlada.";

/// Read-only packaging check; does not create Tauri, load journals or call Magnification.
pub fn installation_check() -> i32 {
    match client::self_check() {
        Ok(()) => {
            println!(
                "{}",
                serde_json::json!({"helperVerified": true, "version": VERSION, "realDiagnosticEnabled": REAL_DIAGNOSTIC_ENABLED})
            );
            0
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}

pub(crate) fn data_root() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|path| path.join("com.vyra.desktop"))
        .ok_or("No se pudo localizar el directorio de datos de VYRA.".into())
}

pub(crate) fn lease(root: &Path, simulated: bool) -> Result<InstanceGuard, String> {
    use sha2::{Digest, Sha256};
    let name = if simulated {
        format!(
            "Local\\VYRA.Watchdog.Test.{}",
            Sha256::digest(root.to_string_lossy().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    } else {
        "Global\\com.vyra.desktop.Watchdog".into()
    };
    InstanceGuard::acquire(&name)?
        .ok_or("Otro watchdog aún protege una sesión; espera su resultado.".into())
}

fn input() -> mpsc::Receiver<Result<Option<Command>, String>> {
    let (sender, receiver) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(io::stdin());
        loop {
            let frame = protocol::read_frame::<Command>(&mut reader);
            let terminal = !matches!(frame, Ok(Some(_)));
            if sender.send(frame).is_err() || terminal {
                break;
            }
        }
    });
    receiver
}

fn finish<A: MagnificationApi>(controller: &mut session::Controller<A>, cause: &str) -> i32 {
    let outcome = controller.restore(cause);
    let error = outcome.error.as_deref().unwrap_or(cause);
    // At most Ready, Armed, Applied and Finished are emitted; output cannot grow without bound.
    let _ = protocol::send(
        &mut io::stdout(),
        &Event::Finished {
            session: outcome.session,
            restored: outcome.restoration_verified && !outcome.journal_retained,
            accepted: outcome.restoration_accepted,
            verified: outcome.restoration_verified,
            reason: error.chars().take(160).collect(),
        },
    );
    if outcome.error.is_none() { 0 } else { 3 }
}

fn serve<A: MagnificationApi>(
    root: PathBuf,
    api: A,
    simulated: bool,
    recovering: bool,
) -> Result<i32, String> {
    let _lease = lease(&root, simulated)?;
    let mut controller = session::Controller::new(root, api)?;
    let receiver = input();
    let hello = receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Handshake ausente o desconectado.")??;
    let Some(Command::Hello {
        version,
        session,
        parent_pid,
    }) = hello
    else {
        return Err("Se requiere Hello como primera trama.".into());
    };
    if version != VERSION
        || session != controller.journal.session
        || !protocol::valid_session(&session)
    {
        return Err("Versión o sesión IPC no válidas.".into());
    }
    let parent = process::ParentProcess::open(parent_pid)?;
    if !recovering && parent.identity(parent_pid)? != controller.journal.parent {
        return Err("Identidad/fecha del proceso padre no coincide.".into());
    }
    if !recovering && !parent.shares_windows_session(parent_pid)? {
        return Err(
            "El watchdog no comparte una sesión de escritorio interactiva con VYRA.".into(),
        );
    }
    if recovering {
        return Ok(finish(&mut controller, "manualRecovery"));
    }
    if !parent.alive() {
        return Ok(finish(&mut controller, "parentLostBeforeReady"));
    }
    if let Err(error) = controller.prepare_read_only() {
        return Ok(finish(&mut controller, &error));
    }
    if protocol::send(
        &mut io::stdout(),
        &Event::Ready {
            session: session.clone(),
            max_ms: protocol::PRECONFIRM_MS,
            helper_initialized: true,
            helper_read_accepted: true,
            journal_durable: true,
            same_windows_session: true,
        },
    )
    .is_err()
    {
        return Ok(finish(&mut controller, "ipcDisconnectedBeforeReady"));
    }
    let preflight_at = Instant::now();
    let mut effect_armed_at: Option<Instant> = None;
    #[cfg(debug_assertions)]
    let mut controlled_preflight_elapsed = 0;
    #[cfg(not(debug_assertions))]
    let controlled_preflight_elapsed = 0;
    #[cfg(debug_assertions)]
    let mut controlled_effect_elapsed = 0;
    #[cfg(not(debug_assertions))]
    let controlled_effect_elapsed = 0;
    let mut applied_event_sent = false;
    let mut messages = 0u8;
    let mut session = session;
    loop {
        let expired = if let Some(armed_at) = effect_armed_at {
            (armed_at.elapsed().as_millis() as u64).max(controlled_effect_elapsed) >= MAX_MS
        } else {
            (preflight_at.elapsed().as_millis() as u64).max(controlled_preflight_elapsed)
                >= protocol::PRECONFIRM_MS
        };
        if expired {
            return Ok(finish(
                &mut controller,
                if effect_armed_at.is_some() {
                    "timeout"
                } else {
                    "preflightTimeout"
                },
            ));
        }
        if !parent.alive() {
            return Ok(finish(&mut controller, "parentLost"));
        }
        let frame = match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(frame) => frame,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Ok(finish(&mut controller, "ipcDisconnected")),
        };
        let command = match frame {
            Ok(Some(command)) => command,
            Ok(None) => return Ok(finish(&mut controller, "ipcDisconnected")),
            Err(error) => return Ok(finish(&mut controller, &error)),
        };
        messages = messages.saturating_add(1);
        if command.session() != session || messages > 32 {
            return Ok(finish(&mut controller, "invalidSessionOrMessageLimit"));
        }
        // Recheck the active deadline after waiting. A queued command cannot revive an expired session.
        let expired = if let Some(armed_at) = effect_armed_at {
            (armed_at.elapsed().as_millis() as u64).max(controlled_effect_elapsed) >= MAX_MS
        } else {
            (preflight_at.elapsed().as_millis() as u64).max(controlled_preflight_elapsed)
                >= protocol::PRECONFIRM_MS
        };
        if expired {
            return Ok(finish(
                &mut controller,
                if effect_armed_at.is_some() {
                    "timeout"
                } else {
                    "preflightTimeout"
                },
            ));
        }
        match command {
            Command::Arm { .. } if effect_armed_at.is_none() => {
                if !apply_allowed(simulated) {
                    return Ok(finish(&mut controller, "consentIncidentLockdown"));
                }
                if !parent.alive() {
                    return Ok(finish(&mut controller, "parentLostDuringArm"));
                }
                let fresh_session = match controller.arm_after_confirmation() {
                    Ok(session) => session,
                    Err(error) => return Ok(finish(&mut controller, &error)),
                };
                if !parent.alive() {
                    return Ok(finish(&mut controller, "parentLostDuringArm"));
                }
                session = fresh_session;
                effect_armed_at = Some(Instant::now());
                #[cfg(debug_assertions)]
                {
                    controlled_effect_elapsed = 0;
                }
                if protocol::send(
                    &mut io::stdout(),
                    &Event::Armed {
                        session: session.clone(),
                        max_ms: MAX_MS,
                    },
                )
                .is_err()
                {
                    return Ok(finish(&mut controller, "ipcDisconnectedDuringArm"));
                }
            }
            Command::Arm { .. } => {
                return Ok(finish(&mut controller, "duplicateArm"));
            }
            Command::Apply { .. } => {
                let Some(armed_at) = effect_armed_at else {
                    return Ok(finish(&mut controller, "applyBeforeFinalArm"));
                };
                if !apply_allowed(simulated) {
                    return Ok(finish(&mut controller, RECOVERY_BLOCKER));
                }
                if let Err(error) = controller.apply(armed_at + Duration::from_millis(MAX_MS)) {
                    return Ok(finish(&mut controller, &error));
                }
                if !applied_event_sent {
                    applied_event_sent = true;
                    if protocol::send(
                        &mut io::stdout(),
                        &Event::Applied {
                            session: session.clone(),
                            accepted: true,
                            verified: true,
                        },
                    )
                    .is_err()
                    {
                        return Ok(finish(&mut controller, "ipcDisconnected"));
                    }
                }
            }
            Command::Restore { .. } => {
                return Ok(finish(&mut controller, "manualRestore"));
            }
            Command::Cancel { .. } => return Ok(finish(&mut controller, "manualCancel")),
            #[cfg(debug_assertions)]
            Command::Advance { elapsed_ms, .. } if simulated => {
                if effect_armed_at.is_some() {
                    controlled_effect_elapsed = controlled_effect_elapsed.max(elapsed_ms);
                } else {
                    controlled_preflight_elapsed = controlled_preflight_elapsed.max(elapsed_ms);
                }
            }
            _ => return Ok(finish(&mut controller, "invalidCommandState")),
        }
    }
}

pub fn main_entry() -> i32 {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag] if flag == "--self-check" => {
            let check = client::SelfCheck {
                version: VERSION,
                real_diagnostic_enabled: REAL_DIAGNOSTIC_ENABLED,
            };
            protocol::send(&mut io::stdout(), &check).map(|_| 0)
        }
        [flag] if flag == "--run" || flag == "--recover" => data_root()
            .and_then(|root| serve(root, NativeMagnification, false, flag == "--recover")),
        #[cfg(debug_assertions)]
        [flag, root] if flag == "--simulate" || flag == "--simulate-recover" => {
            simulation::safe_root(Path::new(root)).and_then(|root| {
                serve(
                    root.clone(),
                    simulation::FileApi::new(root),
                    true,
                    flag == "--simulate-recover",
                )
            })
        }
        #[cfg(debug_assertions)]
        [flag, root] if flag == "--simulate-main" => simulation::main_fixture(Path::new(root)),
        #[cfg(debug_assertions)]
        [flag, root] if flag == "--simulate-armed-main" => {
            simulation::armed_main_fixture(Path::new(root))
        }
        _ => Err("Uso: vyra-watchdog --self-check | --run | --recover".into()),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}
