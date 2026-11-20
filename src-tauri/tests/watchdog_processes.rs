#![cfg(all(target_os = "windows", debug_assertions))]

use std::{
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command as ProcessCommand, Stdio},
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use vyra_lib::watchdog::{
    protocol::{self, Command, Event, MAX_MS, PRECONFIRM_MS, VERSION},
    simulation::{self, Display},
};

const HELPER: &str = env!("CARGO_BIN_EXE_vyra-watchdog");
static PROCESS_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
struct Fixture {
    root: PathBuf,
    // These cases spawn several helper/parent processes and exercise real OS process handles.
    // Serialize the process fixtures even when Cargo runs unrelated tests in parallel.
    _process_guard: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new() -> Self {
        let process_guard = PROCESS_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("process-test lock poisoned");
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "vyra-watchdog-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        simulation::write(&root, &Display::default()).unwrap();
        Self {
            root,
            _process_guard: process_guard,
        }
    }
    fn prepare(&self) -> String {
        simulation::prepare(&self.root).unwrap()
    }
    fn display(&self) -> Display {
        simulation::read(&self.root).unwrap()
    }
    fn outcome(&self) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Ok(bytes) = fs::read(self.root.join("watchdog-result.json")) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    return value;
                }
            }
            assert!(
                Instant::now() < deadline,
                "No outcome from independent helper"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Isolated test data only; never enumerate/delete arbitrary trees.
        for name in [
            "mock-display.json",
            "mock-display.json.tmp",
            "watchdog-recovery.json",
            "watchdog-recovery.json.tmp",
            "watchdog-result.json",
            "watchdog-result.json.tmp",
        ] {
            let path = self.root.join(name);
            let _ = fs::remove_file(&path);
            let _ = fs::remove_dir(&path);
        }
        let _ = fs::remove_dir(&self.root);
    }
}
struct Peer {
    child: Child,
    stdin: Option<ChildStdin>,
    events: mpsc::Receiver<Result<Event, String>>,
    id: String,
}
impl Peer {
    fn launch(root: &Path, id: &str, recover: bool) -> Self {
        use std::os::windows::process::CommandExt;
        let mut child = ProcessCommand::new(HELPER)
            .arg(if recover {
                "--simulate-recover"
            } else {
                "--simulate"
            })
            .arg(root)
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match protocol::read_frame(&mut reader) {
                    Ok(Some(event)) => {
                        if sender.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(Err("EOF".into()));
                        break;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let mut peer = Self {
            child,
            stdin,
            events,
            id: id.into(),
        };
        peer.send(Command::Hello {
            version: VERSION,
            session: id.into(),
            parent_pid: std::process::id(),
        })
        .unwrap();
        peer
    }
    fn preflight(fixture: &Fixture) -> Self {
        let id = fixture.prepare();
        let peer = Self::launch(&fixture.root, &id, false);
        assert!(matches!(
            peer.event().unwrap(),
            Event::Ready {
                max_ms: PRECONFIRM_MS,
                helper_initialized: true,
                helper_read_accepted: true,
                journal_durable: true,
                same_windows_session: true,
                ..
            }
        ));
        peer
    }
    fn arm(&mut self) {
        let old = self.id.clone();
        self.send(Command::Arm {
            session: old.clone(),
        })
        .unwrap();
        match self.event().unwrap() {
            Event::Armed {
                session,
                max_ms: MAX_MS,
            } if protocol::valid_session(&session) && session != old => {
                self.id = session;
            }
            event => panic!("Unexpected final arm event: {event:?}"),
        }
    }
    fn ready(fixture: &Fixture) -> Self {
        let mut peer = Self::preflight(fixture);
        peer.arm();
        peer
    }
    fn send(&mut self, command: Command) -> Result<(), String> {
        protocol::send(self.stdin.as_mut().ok_or("stdin closed")?, &command)
    }
    fn event(&self) -> Result<Event, String> {
        self.events
            .recv_timeout(Duration::from_secs(4))
            .map_err(|_| "IPC timeout".to_string())?
    }
    fn apply(&mut self) {
        self.send(Command::Apply {
            session: self.id.clone(),
        })
        .unwrap();
        assert!(matches!(
            self.event().unwrap(),
            Event::Applied {
                accepted: true,
                verified: true,
                ..
            }
        ));
    }
    fn restore(&mut self) -> Event {
        self.send(Command::Restore {
            session: self.id.clone(),
        })
        .unwrap();
        self.event().unwrap()
    }
    fn cancel(&mut self) -> Event {
        self.send(Command::Cancel {
            session: self.id.clone(),
        })
        .unwrap();
        self.event().unwrap()
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "Helper did not terminate");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stdin.take();
        // These children only have the fake API. Never kill a production recovery helper.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[test]
fn manual_cancel_before_deadline_is_durable_and_has_no_writes() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join("watchdog-recovery.json")).unwrap())
            .unwrap();
    assert_eq!(journal["state"], "armed");
    assert_eq!(fixture.display().writes, 0);
    assert!(matches!(
        peer.cancel(),
        Event::Finished {
            restored: true,
            accepted: None,
            verified: true,
            ..
        }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert!(!fixture.root.join("watchdog-recovery.json").exists());
    assert_eq!(fixture.outcome()["cause"], "manualCancel");
    let previous = peer.id.clone();
    let id = fixture.prepare();
    assert_ne!(id, previous);
    assert!(protocol::valid_session(&id));
}

#[test]
fn cancel_during_human_confirmation_has_no_writes() {
    let fixture = Fixture::new();
    let mut peer = Peer::preflight(&fixture);
    assert!(matches!(
        peer.cancel(),
        Event::Finished {
            restored: true,
            accepted: None,
            verified: true,
            ..
        }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert_eq!(fixture.outcome()["cause"], "manualCancel");
}

#[test]
fn delayed_confirmation_arms_fresh_nonce_and_fresh_effect_deadline() {
    let fixture = Fixture::new();
    let mut peer = Peer::preflight(&fixture);
    let preflight_nonce = peer.id.clone();
    peer.send(Command::Advance {
        session: preflight_nonce.clone(),
        elapsed_ms: PRECONFIRM_MS - 1_000,
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(fixture.display().writes, 0);
    peer.arm();
    assert_ne!(peer.id, preflight_nonce);
    peer.apply();
    peer.send(Command::Advance {
        session: peer.id.clone(),
        elapsed_ms: MAX_MS,
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished {
            restored: true,
            accepted: Some(true),
            verified: true,
            ..
        }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 2);
    assert_eq!(fixture.outcome()["cause"], "timeout");
}

#[test]
fn stale_preflight_nonce_cannot_apply_after_final_arm() {
    let fixture = Fixture::new();
    let mut peer = Peer::preflight(&fixture);
    let stale_nonce = peer.id.clone();
    peer.arm();
    peer.send(Command::Apply {
        session: stale_nonce,
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert_eq!(fixture.outcome()["cause"], "invalidSessionOrMessageLimit");
}

#[test]
fn expired_human_confirmation_window_finishes_without_apply() {
    let fixture = Fixture::new();
    let mut peer = Peer::preflight(&fixture);
    peer.send(Command::Advance {
        session: peer.id.clone(),
        elapsed_ms: PRECONFIRM_MS,
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished {
            restored: true,
            accepted: None,
            verified: true,
            ..
        }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert_eq!(fixture.outcome()["cause"], "preflightTimeout");
}

#[test]
fn controlled_ten_second_deadline_restores_in_independent_process() {
    let fixture = Fixture::new();
    let original = fixture.display().matrix;
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    peer.send(Command::Advance {
        session: peer.id.clone(),
        elapsed_ms: MAX_MS,
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished {
            restored: true,
            accepted: Some(true),
            verified: true,
            ..
        }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().matrix, original);
    assert_eq!(fixture.display().writes, 2);
    assert_eq!(fixture.outcome()["cause"], "timeout");
}

#[test]
fn expiration_before_apply_cannot_be_extended_or_write_late() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.send(Command::Advance {
        session: peer.id.clone(),
        elapsed_ms: MAX_MS,
    })
    .unwrap();
    let _ = peer.send(Command::Apply {
        session: peer.id.clone(),
    });
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
}

#[test]
fn duplicate_apply_and_restore_do_not_duplicate_effects() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    peer.send(Command::Apply {
        session: peer.id.clone(),
    })
    .unwrap();
    peer.send(Command::Restore {
        session: peer.id.clone(),
    })
    .unwrap();
    let _ = peer.send(Command::Restore {
        session: peer.id.clone(),
    });
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 2);
}

#[test]
fn disconnect_with_parent_still_alive_restores() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    peer.stdin.take();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.outcome()["cause"], "ipcDisconnected");
    assert_eq!(fixture.display().matrix, Display::default().matrix);
}

#[test]
fn killed_helper_preserves_pending_journal_and_restart_can_recover() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    peer.child.kill().unwrap();
    peer.child.wait().unwrap();
    assert!(fixture.root.join("watchdog-recovery.json").exists());
    assert!(simulation::prepare(&fixture.root).is_err());
    let mut recovery = Peer::launch(&fixture.root, &peer.id, true);
    assert!(matches!(
        recovery.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(recovery.wait().success());
    assert_eq!(fixture.display().matrix, Display::default().matrix);
    assert!(!fixture.root.join("watchdog-recovery.json").exists());
}

#[test]
fn external_matrix_is_not_overwritten() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    let mut external = fixture.display();
    external.matrix[0][0] = 0.37;
    simulation::write(&fixture.root, &external).unwrap();
    assert!(matches!(
        peer.restore(),
        Event::Finished {
            restored: false,
            accepted: None,
            ..
        }
    ));
    assert!(!peer.wait().success());
    assert_eq!(fixture.display().matrix, external.matrix);
    assert_eq!(fixture.display().writes, 1);
    assert!(fixture.root.join("watchdog-recovery.json").exists());
}

#[test]
fn corrupt_journal_during_handshake_is_preserved_without_writes() {
    let fixture = Fixture::new();
    let id = fixture.prepare();
    fs::write(fixture.root.join("watchdog-recovery.json"), b"corrupt").unwrap();
    let mut peer = Peer::launch(&fixture.root, &id, false);
    assert!(peer.event().is_err());
    assert!(!peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert_eq!(
        fs::read(fixture.root.join("watchdog-recovery.json")).unwrap(),
        b"corrupt"
    );
}

#[test]
fn persistence_failure_after_ready_prevents_apply_and_keeps_journal() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    fs::create_dir(fixture.root.join("watchdog-recovery.json.tmp")).unwrap();
    peer.send(Command::Apply {
        session: peer.id.clone(),
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished {
            restored: false,
            verified: true,
            ..
        }
    ));
    assert!(!peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert!(fixture.root.join("watchdog-recovery.json").exists());
}

#[test]
fn restore_failure_is_recorded_and_explicit_recovery_can_retry() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.apply();
    let mut display = fixture.display();
    display.fail_write = true;
    simulation::write(&fixture.root, &display).unwrap();
    assert!(matches!(
        peer.restore(),
        Event::Finished {
            restored: false,
            accepted: Some(false),
            ..
        }
    ));
    assert!(!peer.wait().success());
    assert_eq!(fixture.outcome()["restorationAccepted"], false);
    display.fail_write = false;
    simulation::write(&fixture.root, &display).unwrap();
    let mut recovered = Peer::launch(&fixture.root, &peer.id, true);
    assert!(matches!(
        recovered.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(recovered.wait().success());
}

#[test]
fn accepted_but_discrepant_application_is_not_reported_as_verified() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    let mut display = fixture.display();
    display.bad_readback = true;
    simulation::write(&fixture.root, &display).unwrap();
    peer.send(Command::Apply {
        session: peer.id.clone(),
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    let result = fixture.outcome();
    assert_eq!(result["applicationAccepted"], true);
    assert_eq!(result["applicationVerified"], false);
}

#[test]
fn wrong_session_is_rejected_without_color_write() {
    let fixture = Fixture::new();
    let mut peer = Peer::ready(&fixture);
    peer.send(Command::Apply {
        session: "0".repeat(64),
    })
    .unwrap();
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished { restored: true, .. }
    ));
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
}

#[test]
fn second_helper_cannot_take_over_active_session() {
    let fixture = Fixture::new();
    let mut first = Peer::ready(&fixture);
    let mut second = Peer::launch(&fixture.root, &first.id, false);
    assert!(second.event().is_err());
    assert!(!second.wait().success());
    assert!(matches!(
        first.restore(),
        Event::Finished { restored: true, .. }
    ));
    assert!(first.wait().success());
}

#[test]
fn actual_parent_process_termination_triggers_independent_restore() {
    use std::os::windows::process::CommandExt;
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let fixture = Fixture::new();
    let mut parent = ProcessCommand::new(HELPER)
        .args(["--simulate-main", fixture.root.to_str().unwrap()])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = parent.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(protocol::read_frame::<serde_json::Value>(
            &mut BufReader::new(stdout),
        ));
    });
    let result = receiver.recv_timeout(Duration::from_secs(5));
    if result.is_err() {
        let _ = parent.kill();
        let _ = parent.wait();
        panic!("Fixture parent never applied");
    }
    let ready = result.unwrap().unwrap().unwrap();
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE,
            false,
            ready["helperPid"].as_u64().unwrap() as u32,
        )
    }
    .unwrap();
    parent.kill().unwrap();
    parent.wait().unwrap();
    let outcome = fixture.outcome();
    assert_eq!(outcome["restorationVerified"], true);
    assert!(["parentLost", "ipcDisconnected"].contains(&outcome["cause"].as_str().unwrap()));
    assert_eq!(fixture.display().matrix, Display::default().matrix);
    assert_eq!(unsafe { WaitForSingleObject(handle, 2000) }, WAIT_OBJECT_0);
    unsafe {
        CloseHandle(handle).unwrap();
    }
}

#[test]
fn parent_loss_immediately_after_final_arm_cannot_apply_color() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let mut parent = ProcessCommand::new(HELPER)
        .args(["--simulate-armed-main", fixture.root.to_str().unwrap()])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = parent.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(protocol::read_frame::<serde_json::Value>(
            &mut BufReader::new(stdout),
        ));
    });
    let ready = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("armed fixture timeout")
        .unwrap()
        .unwrap();
    assert!(protocol::valid_session(ready["session"].as_str().unwrap()));
    parent.kill().unwrap();
    parent.wait().unwrap();
    let outcome = fixture.outcome();
    assert_eq!(outcome["restorationVerified"], true);
    assert!(["parentLost", "ipcDisconnected"].contains(&outcome["cause"].as_str().unwrap()));
    assert_eq!(fixture.display().matrix, Display::default().matrix);
    assert_eq!(fixture.display().writes, 0);
}

#[test]
fn absent_helper_preserves_prepared_journal_without_application() {
    let fixture = Fixture::new();
    fixture.prepare();
    assert!(simulation::missing_helper(&fixture.root).is_err());
    assert!(fixture.root.join("watchdog-recovery.json").exists());
    assert_eq!(fixture.display().writes, 0);
}

#[test]
fn original_read_failure_rejects_handshake_and_retains_evidence() {
    let fixture = Fixture::new();
    let id = fixture.prepare();
    let mut display = fixture.display();
    display.fail_read = true;
    simulation::write(&fixture.root, &display).unwrap();
    let mut peer = Peer::launch(&fixture.root, &id, false);
    assert!(matches!(
        peer.event().unwrap(),
        Event::Finished {
            restored: false,
            verified: false,
            ..
        }
    ));
    assert!(!peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert!(fixture.root.join("watchdog-recovery.json").exists());
}

#[test]
fn close_during_ready_handshake_never_applies_an_effect() {
    let fixture = Fixture::new();
    let id = fixture.prepare();
    let mut peer = Peer::launch(&fixture.root, &id, false);
    peer.stdin.take();
    let first = peer.event().unwrap();
    if matches!(first, Event::Ready { .. }) {
        assert!(matches!(
            peer.event().unwrap(),
            Event::Finished { restored: true, .. }
        ));
    } else {
        assert!(matches!(first, Event::Finished { restored: true, .. }));
    }
    assert!(peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
}

#[test]
fn reused_or_forged_parent_identity_is_rejected_before_ready() {
    let fixture = Fixture::new();
    let id = fixture.prepare();
    let path = fixture.root.join("watchdog-recovery.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["parent"]["created"] = serde_json::json!(1);
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let mut peer = Peer::launch(&fixture.root, &id, false);
    assert!(peer.event().is_err());
    assert!(!peer.wait().success());
    assert_eq!(fixture.display().writes, 0);
    assert!(path.exists());
}
