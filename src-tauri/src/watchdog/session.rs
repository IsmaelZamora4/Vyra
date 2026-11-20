use super::{
    process::ParentIdentity,
    protocol::{MAX_MS, valid_session},
};
use crate::{
    color_engine::{
        magnification::{MagnificationApi, Matrix, matrices_match, restore_known},
        matrix_for,
    },
    models::Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub const JOURNAL: &str = "watchdog-recovery.json";
pub const RESULT: &str = "watchdog-result.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Prepared,
    Armed,
    Applying,
    Applied,
    Restoring,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub version: u8,
    pub session: String,
    pub parent: ParentIdentity,
    pub original: Matrix,
    pub target: Matrix,
    pub max_ms: u64,
    pub state: State,
    pub may_have_written: bool,
}
impl Journal {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !valid_session(&self.session)
            || self.parent.pid == 0
            || self.parent.created == 0
            || self.max_ms != MAX_MS
            || self.original.iter().flatten().any(|v| !v.is_finite())
            || self.target != target()
            || (matches!(self.state, State::Prepared | State::Armed) && self.may_have_written)
            || (matches!(self.state, State::Applying | State::Applied) && !self.may_have_written)
        {
            return Err("Diario watchdog inválido o versión/plan no permitido.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Outcome {
    pub session: String,
    pub cause: String,
    pub application_accepted: Option<bool>,
    pub application_verified: Option<bool>,
    pub restoration_accepted: Option<bool>,
    pub restoration_verified: bool,
    pub journal_retained: bool,
    pub error: Option<String>,
}

pub fn target() -> Matrix {
    matrix_for(&Settings {
        saturation: 5,
        ..Settings::default()
    })
}

fn fresh_session() -> Result<String, String> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| format!("No se pudo crear nonce seguro: {e}"))?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn load(root: &Path) -> Result<Journal, String> {
    let path = root.join(JOURNAL);
    let size = fs::metadata(&path)
        .map_err(|e| format!("Diario watchdog no legible: {e}"))?
        .len();
    if size > 8192 {
        return Err("Diario watchdog demasiado grande.".into());
    }
    let journal: Journal = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Diario watchdog corrupto: {e}"))?;
    journal.validate()?;
    Ok(journal)
}

pub fn prepare<A: MagnificationApi>(root: &Path, api: &mut A) -> Result<Journal, String> {
    if root.join(JOURNAL).try_exists().map_err(|e| e.to_string())? {
        return Err("Recuperación watchdog pendiente; no se reemplazó el diario.".into());
    }
    api.initialize()?;
    let original = api.read();
    let cleanup = api.uninitialize();
    let original = match (original, cleanup) {
        (Ok(original), Ok(())) => original,
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let journal = Journal {
        version: 1,
        session: fresh_session()?,
        parent: ParentIdentity::current()?,
        original,
        target: target(),
        max_ms: MAX_MS,
        state: State::Prepared,
        may_have_written: false,
    };
    journal.validate()?;
    crate::durable::json(&root.join(JOURNAL), &journal)?;
    Ok(journal)
}

pub struct Controller<A: MagnificationApi> {
    pub journal: Journal,
    pub outcome: Outcome,
    api: A,
    root: PathBuf,
    initialized: bool,
    finished: bool,
}
impl<A: MagnificationApi> Controller<A> {
    pub fn new(root: PathBuf, api: A) -> Result<Self, String> {
        let journal = load(&root)?;
        let outcome = Outcome {
            session: journal.session.clone(),
            cause: "prepared".into(),
            application_accepted: None,
            application_verified: None,
            restoration_accepted: None,
            restoration_verified: false,
            journal_retained: true,
            error: None,
        };
        Ok(Self {
            journal,
            outcome,
            api,
            root,
            initialized: false,
            finished: false,
        })
    }
    pub fn prepare_read_only(&mut self) -> Result<(), String> {
        if self.journal.state != State::Prepared || self.journal.may_have_written {
            return Err("La sesión de preflight no está en estado Prepared.".into());
        }
        self.api.initialize()?;
        self.initialized = true;
        if !matrices_match(self.api.read()?, self.journal.original) {
            return Err("El efecto cambió antes del handshake; no se aplicará color.".into());
        }
        Ok(())
    }
    pub fn arm_after_confirmation(&mut self) -> Result<String, String> {
        if self.journal.state != State::Prepared || !self.initialized {
            return Err("La sesión ya venció o no completó el preflight.".into());
        }
        let latest = self.api.read()?;
        if !matrices_match(latest, self.journal.original) {
            return Err(
                "Conflicto externo: la matriz cambió antes del armado; no se aplicará color."
                    .into(),
            );
        }
        let mut next = self.journal.clone();
        next.session = fresh_session()?;
        next.original = latest;
        next.state = State::Armed;
        self.persist(&next)?;
        self.journal = next;
        self.outcome.session = self.journal.session.clone();
        Ok(self.journal.session.clone())
    }
    fn persist(&self, journal: &Journal) -> Result<(), String> {
        // Never replace evidence belonging to a different or corrupt session.
        if load(&self.root)?.session != self.journal.session {
            return Err("El diario cambió de sesión.".into());
        }
        crate::durable::json(&self.root.join(JOURNAL), journal)
    }
    pub fn apply(&mut self, deadline: Instant) -> Result<(), String> {
        if self.finished || Instant::now() >= deadline {
            return Err("Plazo agotado; aplicación rechazada.".into());
        }
        if self.journal.state == State::Applied {
            return Ok(());
        }
        if self.journal.state != State::Armed {
            return Err("Aplicación fuera de estado Armed.".into());
        }
        if !matrices_match(self.api.read()?, self.journal.original) {
            return Err("Conflicto externo antes de aplicar.".into());
        }
        if Instant::now() >= deadline {
            return Err("Plazo agotado durante la lectura; aplicación rechazada.".into());
        }
        let mut next = self.journal.clone();
        next.state = State::Applying;
        next.may_have_written = true;
        self.persist(&next)?;
        self.journal = next;
        if Instant::now() >= deadline {
            return Err("Plazo agotado antes de escribir; aplicación rechazada.".into());
        }
        self.outcome.application_accepted = Some(false);
        self.api.write(self.journal.target)?;
        self.outcome.application_accepted = Some(true);
        if !matrices_match(self.api.read()?, self.journal.target) {
            self.outcome.application_verified = Some(false);
            return Err("Lectura discrepante después de aplicar.".into());
        }
        self.outcome.application_verified = Some(true);
        let mut next = self.journal.clone();
        next.state = State::Applied;
        self.persist(&next)?;
        self.journal = next;
        Ok(())
    }
    pub fn restore(&mut self, cause: &str) -> Outcome {
        if self.finished {
            return self.outcome.clone();
        }
        self.outcome.cause = cause.into();
        let mut evidence_error = None;
        let mut next = self.journal.clone();
        next.state = State::Restoring;
        if let Err(error) = self.persist(&next) {
            evidence_error = Some(error);
        }
        let result = (|| -> Result<(), String> {
            if !self.initialized {
                self.api.initialize()?;
                self.initialized = true;
            }
            let known = if self.journal.may_have_written {
                vec![self.journal.target]
            } else {
                Vec::new()
            };
            restore_known(
                &mut self.api,
                self.journal.original,
                &known,
                &mut self.outcome.restoration_accepted,
            )?;
            self.outcome.restoration_verified = true;
            self.api.uninitialize()?;
            self.initialized = false;
            Ok(())
        })();
        self.outcome.error = result.err().or(evidence_error);
        if self.outcome.error.is_none() {
            self.outcome.journal_retained = false;
            if let Err(error) = crate::durable::json(&self.root.join(RESULT), &self.outcome) {
                self.outcome.error = Some(error);
                self.outcome.journal_retained = true;
            } else if let Err(error) = fs::remove_file(self.root.join(JOURNAL)) {
                self.outcome.error = Some(format!("Restaurado; diario conservado: {error}"));
                self.outcome.journal_retained = true;
            }
        }
        if self.outcome.error.is_some() {
            let mut failed = self.journal.clone();
            failed.state = State::Failed;
            let _ = self.persist(&failed);
            let _ = crate::durable::json(&self.root.join(RESULT), &self.outcome);
        }
        self.finished = true;
        self.outcome.clone()
    }
}
impl<A: MagnificationApi> Drop for Controller<A> {
    fn drop(&mut self) {
        // A process exit still closes resources if an API failed. The journal remains authoritative.
        if self.initialized {
            let _ = self.api.uninitialize();
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
