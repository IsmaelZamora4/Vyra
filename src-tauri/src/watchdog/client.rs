use super::{
    protocol::{self, Command, Event, PRECONFIRM_MS, VERSION},
    session::{self, Journal},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command as ProcessCommand, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SelfCheck {
    pub version: u8,
    pub real_diagnostic_enabled: bool,
}

fn hidden(command: &mut ProcessCommand) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW; inherited pipes remain usable.
    }
}

pub(crate) fn verified_helper() -> Result<PathBuf, String> {
    let directory = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Ejecutable VYRA sin directorio.")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let helper = directory.join("vyra-watchdog.exe");
    verify_at(&directory, &helper, env!("VYRA_WATCHDOG_SHA256"))?;
    Ok(helper)
}

fn verify_at(directory: &Path, helper: &Path, expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let metadata = fs::symlink_metadata(helper).map_err(|e| format!("Watchdog ausente: {e}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || expected.len() != 64 {
        return Err("Watchdog no verificado para este build.".into());
    }
    let canonical = helper.canonicalize().map_err(|e| e.to_string())?;
    if canonical.parent() != Some(directory) {
        return Err("Watchdog fuera del directorio de VYRA.".into());
    }
    let actual: String = Sha256::digest(fs::read(&canonical).map_err(|e| e.to_string())?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        return Err("SHA-256 del watchdog no coincide con el helper empaquetado.".into());
    }
    Ok(())
}

pub(crate) fn self_check() -> Result<(), String> {
    let path = verified_helper()?;
    let mut command = ProcessCommand::new(path);
    command
        .arg("--self-check")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hidden(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("Sin stdout del helper.")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(protocol::read_frame::<SelfCheck>(&mut BufReader::new(
            stdout,
        )));
    });
    let response = receiver.recv_timeout(Duration::from_secs(2));
    if response.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Watchdog no respondió a self-check.".into());
    }
    let response = response.unwrap()?;
    let deadline = Instant::now() + Duration::from_secs(1);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Self-check no finalizó.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err("Self-check terminó con error.".into());
    }
    match response {
        Some(check)
            if check.version == VERSION
                && check.real_diagnostic_enabled == super::REAL_DIAGNOSTIC_ENABLED =>
        {
            Ok(())
        }
        _ => Err("Self-check de versión/política inválido.".into()),
    }
}

pub(crate) struct Client {
    pub child: Child,
    pub session: String,
    stdin: Option<ChildStdin>,
    events: mpsc::Receiver<Result<Event, String>>,
    pub state: String,
    pub last_error: Option<String>,
    pub finished: bool,
    pub read_only_preflight_passed: bool,
    simulated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyRequestError {
    RejectedBeforeSend(String),
    MayHaveBeenSent(String),
}
impl ApplyRequestError {
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::RejectedBeforeSend(reason) | Self::MayHaveBeenSent(reason) => reason,
        }
    }
    pub(crate) fn may_have_been_sent(&self) -> bool {
        matches!(self, Self::MayHaveBeenSent(_))
    }
}
impl std::fmt::Display for ApplyRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.reason())
    }
}
impl std::error::Error for ApplyRequestError {}
impl Client {
    pub fn prepare_native(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if root
            != super::data_root()?
                .canonicalize()
                .map_err(|e| e.to_string())?
        {
            return Err("Directorio de datos no corresponde a VYRA.".into());
        }
        let helper = verified_helper()?;
        let journal = {
            let _lease = super::lease(&root, false)?;
            session::prepare(
                &root,
                &mut crate::color_engine::magnification::NativeMagnification,
            )?
        };
        Self::spawn(&helper, &root, &journal, false, false)
    }
    pub fn recover_native(root: &Path) -> Result<Self, String> {
        if root.canonicalize().map_err(|e| e.to_string())?
            != super::data_root()?
                .canonicalize()
                .map_err(|e| e.to_string())?
        {
            return Err("Directorio de recuperación no corresponde a VYRA.".into());
        }
        let helper = verified_helper()?;
        Self::spawn(&helper, root, &session::load(root)?, false, true)
    }
    pub(crate) fn spawn(
        helper: &Path,
        root: &Path,
        journal: &Journal,
        simulated: bool,
        recovering: bool,
    ) -> Result<Self, String> {
        let mut command = ProcessCommand::new(helper);
        if simulated {
            if !cfg!(debug_assertions) {
                return Err("Simulación no incluida en release.".into());
            }
            command
                .arg(if recovering {
                    "--simulate-recover"
                } else {
                    "--simulate"
                })
                .arg(root);
        } else {
            command.arg(if recovering { "--recover" } else { "--run" });
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hidden(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("No se pudo iniciar watchdog: {e}"))?;
        let stdin = child.stdin.take().ok_or("Watchdog sin stdin.")?;
        let stdout = child.stdout.take().ok_or("Watchdog sin stdout.")?;
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match protocol::read_frame::<Event>(&mut reader) {
                    Ok(Some(event)) => {
                        if sender.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let mut client = Self {
            child,
            session: journal.session.clone(),
            stdin: Some(stdin),
            events,
            state: "starting".into(),
            last_error: None,
            finished: false,
            read_only_preflight_passed: false,
            simulated,
        };
        client.send(&Command::Hello {
            version: VERSION,
            session: journal.session.clone(),
            parent_pid: std::process::id(),
        })?;
        if !recovering {
            match client
                .events
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Handshake watchdog no confirmado.")??
            {
                Event::Ready {
                    session,
                    max_ms,
                    helper_initialized: true,
                    helper_read_accepted: true,
                    journal_durable: true,
                    same_windows_session: true,
                } if session == client.session && max_ms == PRECONFIRM_MS => {
                    client.state = "preflight".into();
                    client.read_only_preflight_passed = true;
                }
                Event::Finished { reason, .. } => return Err(reason),
                _ => return Err("Watchdog no confirmó Ready con el plan esperado.".into()),
            }
        }
        Ok(client)
    }
    pub fn send(&mut self, command: &Command) -> Result<(), String> {
        if command.session() != self.session {
            return Err("Sesión IPC diferente.".into());
        }
        protocol::send(self.stdin.as_mut().ok_or("IPC cerrado.")?, command)
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let mut received = Vec::new();
        while let Ok(result) = self.events.try_recv() {
            match result {
                Ok(event) => {
                    let session = match &event {
                        Event::Ready { session, .. }
                        | Event::Armed { session, .. }
                        | Event::Applied { session, .. }
                        | Event::Finished { session, .. } => session,
                    };
                    if session != &self.session {
                        self.last_error = Some("Respuesta watchdog de una sesión distinta.".into());
                        self.stdin.take();
                        continue;
                    }
                    match &event {
                        Event::Ready { .. } => self.state = "preflight".into(),
                        Event::Armed { .. } => self.state = "armed".into(),
                        Event::Applied { .. } => self.state = "applied".into(),
                        Event::Finished {
                            restored, reason, ..
                        } => {
                            self.state = if *restored { "restored" } else { "failed" }.into();
                            self.finished = true;
                            if !restored {
                                self.last_error = Some(reason.clone());
                            }
                            self.stdin.take();
                        }
                    }
                    received.push(event);
                }
                Err(error) => self.last_error = Some(error),
            }
        }
        match self.child.try_wait() {
            Ok(Some(status)) if !self.finished => {
                // Drain once more after exit: the pipe reader may still be delivering its final frame.
                self.state = "exited".into();
                self.last_error = Some(format!("Watchdog finalizó sin confirmación: {status}"));
            }
            Err(error) => {
                self.state = "exited".into();
                self.last_error = Some(format!("No se pudo consultar el watchdog: {error}"));
            }
            _ => (),
        }
        received
    }
    pub fn request_restore(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.send(&Command::Restore {
            session: self.session.clone(),
        })
    }
    pub fn request_cancel(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.send(&Command::Cancel {
            session: self.session.clone(),
        })
    }
    pub fn arm_after_confirmation(&mut self) -> Result<(), String> {
        if !super::apply_allowed(self.simulated) {
            return Err(super::RECOVERY_BLOCKER.into());
        }
        if !self.read_only_preflight_passed || self.finished || self.state != "preflight" {
            return Err("Watchdog ya no conserva un preflight válido.".into());
        }
        let previous_session = self.session.clone();
        self.send(&Command::Arm {
            session: previous_session.clone(),
        })?;
        match self
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Watchdog no confirmó el armado final.".to_string())??
        {
            Event::Armed { session, max_ms }
                if protocol::valid_session(&session)
                    && session != previous_session
                    && max_ms == protocol::MAX_MS =>
            {
                self.session = session;
                self.state = "armed".into();
                Ok(())
            }
            Event::Finished {
                restored, reason, ..
            } => {
                self.finished = true;
                self.state = if restored { "restored" } else { "failed" }.into();
                self.last_error = Some(reason.clone());
                Err(reason)
            }
            _ => Err("Watchdog no confirmó un armado nuevo con el plan esperado.".into()),
        }
    }
    pub fn request_apply(&mut self) -> Result<(), ApplyRequestError> {
        if !super::apply_allowed(self.simulated) {
            return Err(ApplyRequestError::RejectedBeforeSend(
                super::RECOVERY_BLOCKER.into(),
            ));
        }
        if !self.read_only_preflight_passed || self.finished || self.state != "armed" {
            return Err(ApplyRequestError::RejectedBeforeSend(
                "Watchdog no está armado para solicitar la prueba.".into(),
            ));
        }
        match self
            .child
            .try_wait()
            .map_err(|e| ApplyRequestError::RejectedBeforeSend(e.to_string()))?
        {
            Some(status) => {
                self.finished = true;
                self.state = "exited".into();
                Err(ApplyRequestError::RejectedBeforeSend(format!(
                    "Watchdog ya finalizó antes de Apply: {status}"
                )))
            }
            None => {
                self.send(&Command::Apply {
                    session: self.session.clone(),
                })
                .map_err(ApplyRequestError::MayHaveBeenSent)?;
                self.state = "applying".into();
                Ok(())
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        // Never kill an armed recovery helper. EOF or the independent deadline ends its session.
        self.stdin.take();
        let _ = self.child.try_wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_wrong_helper_is_rejected_without_launching() {
        let directory = std::env::temp_dir().canonicalize().unwrap();
        assert!(verify_at(&directory, &directory.join("vyra-missing-helper.exe"), "").is_err());
        let path = directory.join(format!("vyra-helper-integrity-{}.exe", std::process::id()));
        fs::write(&path, b"wrong helper").unwrap();
        assert!(
            verify_at(&directory, &path, &"0".repeat(64))
                .unwrap_err()
                .contains("SHA-256")
        );
        fs::remove_file(path).unwrap();
    }
}
