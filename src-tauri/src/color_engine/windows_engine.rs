use super::{Capabilities, ColorEngine, EngineStatus, matrix_for};
use crate::models::Settings;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use super::magnification::{
    MagnificationApi, Matrix, NativeMagnification, matrices_match, restore_known,
};
#[cfg(test)]
use super::magnification::{matrix_from_transform, transform_from_matrix};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryJournal {
    version: u32,
    original: Matrix,
    #[serde(default)]
    applied: Option<Matrix>,
    #[serde(default)]
    pending: Option<Matrix>,
}
pub struct WindowsColorEngine<A: MagnificationApi = NativeMagnification> {
    api: A,
    journal: Option<RecoveryJournal>,
    recovery_file: Option<PathBuf>,
    initialized: bool,
    enabled: bool,
    recovery_required: bool,
    error: Option<String>,
    available: bool,
}
impl WindowsColorEngine {
    pub fn new(recovery_file: Option<PathBuf>) -> Self {
        Self::with_api(recovery_file, NativeMagnification)
    }
}
impl<A: MagnificationApi> WindowsColorEngine<A> {
    pub(super) fn with_api(recovery_file: Option<PathBuf>, api: A) -> Self {
        let mut engine = Self {
            api,
            journal: None,
            recovery_file,
            initialized: false,
            enabled: false,
            recovery_required: false,
            error: None,
            available: false,
        };
        if let Some(path) = &engine.recovery_file {
            // try_exists surfaces access failures instead of treating them as no journal.
            match path.try_exists() {
                Ok(false) => (),
                Ok(true) => {
                    engine.recovery_required = true;
                    match fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
                        serde_json::from_slice::<RecoveryJournal>(&bytes).map_err(|e| e.to_string())
                    }) {
                        Ok(journal) if Self::valid_journal(&journal) => {
                            engine.journal = Some(journal)
                        }
                        Ok(_) => {
                            engine.error =
                                Some("Diario de recuperación inválido: versión o matriz.".into())
                        }
                        Err(error) => {
                            engine.error = Some(format!(
                                "Diario de recuperación inválido o ilegible: {error}"
                            ))
                        }
                    }
                }
                Err(error) => {
                    engine.recovery_required = true;
                    engine.error = Some(format!("No se pudo consultar el diario: {error}"));
                }
            }
        }
        match engine.read_current() {
            Ok(_) => engine.available = true,
            Err(error) if engine.error.is_none() => engine.error = Some(error),
            Err(_) => (),
        }
        engine
    }
    fn valid_journal(journal: &RecoveryJournal) -> bool {
        journal.version == 1
            && std::iter::once(&journal.original)
                .chain(journal.applied.iter())
                .chain(journal.pending.iter())
                .all(|m| m.iter().flatten().all(|v| v.is_finite()))
    }
    fn persist(&self, journal: &RecoveryJournal) -> Result<(), String> {
        let path = self
            .recovery_file
            .as_ref()
            .ok_or("No se configuró ruta de recuperación.")?;
        crate::durable::json(path, journal)
            .map_err(|error| format!("No se pudo guardar el diario de recuperación: {error}"))
    }
    fn ensure_initialized(&mut self) -> Result<(), String> {
        if !self.initialized {
            self.api.initialize()?;
            self.initialized = true;
        }
        Ok(())
    }
    fn release(&mut self) -> Result<(), String> {
        if self.initialized {
            if let Err(error) = self.api.uninitialize() {
                self.error = Some(error.clone());
                return Err(error);
            }
            self.initialized = false;
        }
        Ok(())
    }
    pub(super) fn read_current(&mut self) -> Result<Matrix, String> {
        self.ensure_initialized()?;
        let result = self.api.read();
        if !self.enabled {
            let cleanup = self.release();
            match (result, cleanup) {
                (Ok(matrix), Ok(())) => Ok(matrix),
                (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
                (Err(error), Err(cleanup)) => Err(format!("{error} {cleanup}")),
            }
        } else {
            result
        }
    }
    fn expected_state(journal: &RecoveryJournal, current: Matrix) -> bool {
        matrices_match(current, journal.original)
            || journal.applied.is_some_and(|m| matrices_match(current, m))
            || journal.pending.is_some_and(|m| matrices_match(current, m))
    }
    fn restore(&mut self) -> Result<(), String> {
        let Some(journal) = self.journal.clone() else {
            if self.recovery_required || self.enabled {
                return Err(self
                    .error
                    .clone()
                    .unwrap_or("Estado inconsistente: no hay matriz original válida.".into()));
            }
            return self.release();
        };
        self.ensure_initialized()?;
        let known: Vec<Matrix> = journal
            .applied
            .iter()
            .chain(journal.pending.iter())
            .copied()
            .collect();
        restore_known(&mut self.api, journal.original, &known, &mut None)?;
        // Keep the journal until both readback and runtime cleanup succeed.
        self.release()?;
        if let Some(path) = &self.recovery_file {
            match fs::remove_file(path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => {
                    return Err(format!(
                        "Colores restaurados; diario conservado por error: {error}"
                    ));
                }
            }
        }
        self.journal = None;
        self.enabled = false;
        self.recovery_required = false;
        self.error = None;
        Ok(())
    }
}
impl<A: MagnificationApi> ColorEngine for WindowsColorEngine<A> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            engine_name: "Windows Magnification API".into(), selected_backend: "windows".into(),
            backend_preference: crate::models::BackendPreference::Windows,
            is_simulation: false, available: self.available,
            supported_controls: vec!["brightness".into(), "contrast".into(), "saturation".into(), "hue".into()],
            unsupported_controls: vec!["gamma".into(), "sharpness".into()],
            per_monitor: false, can_restore: true, detected_adapters: Vec::new(),
            limitation: "API experimental global; disponibilidad comprobada al leer/inicializar. HDR, juegos y varios monitores sin validar. La última escritura de cualquier aplicación sustituye la matriz global.".into(),
            activation_blocked_reason: Some(crate::diagnostic::RECOVERY_BLOCKER.into()),
        }
    }
    fn initialize(&mut self) -> Result<(), String> {
        // A public initialization request is read-only and never replaces the original.
        self.read_current().map(|_| ())
    }
    fn diagnostic_read_current(&mut self) -> Result<Matrix, String> {
        self.read_current()
    }
    fn get_current_state(&self) -> EngineStatus {
        EngineStatus {
            enabled: self.enabled,
            recovery_required: self.recovery_required,
            description: self.error.clone().unwrap_or_else(|| {
                if self.recovery_required {
                    "Restauración pendiente; diario conservado."
                } else if self.enabled {
                    "Filtro activo; original guardado."
                } else {
                    "Filtros apagados."
                }
                .into()
            }),
        }
    }
    fn apply(&mut self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        if self.recovery_required {
            return Err("Restaura el estado pendiente antes de aplicar filtros.".into());
        }
        if self.enabled && self.journal.is_none() {
            self.recovery_required = true;
            return Err("Estado inconsistente: motor activo sin matriz original.".into());
        }
        self.ensure_initialized()?;
        let current = match self.api.read() {
            Ok(matrix) => matrix,
            Err(error) => {
                if self.enabled {
                    self.recovery_required = true;
                } else if let Err(cleanup) = self.release() {
                    return Err(format!("{error} {cleanup}"));
                }
                return Err(error);
            }
        };
        let mut journal = match self.journal.clone() {
            Some(journal) => {
                if !Self::expected_state(&journal, current) {
                    self.recovery_required = true;
                    self.error =
                        Some("Conflicto: la matriz global fue modificada externamente.".into());
                    return Err(self.error.clone().unwrap());
                }
                journal
            }
            None => {
                if self
                    .recovery_file
                    .as_ref()
                    .is_some_and(|path| path.exists())
                {
                    self.recovery_required = true;
                    return Err("Apareció un diario pendiente; reinicia VYRA para leerlo. No se aplicó color.".into());
                }
                RecoveryJournal {
                    version: 1,
                    original: current,
                    applied: None,
                    pending: None,
                }
            }
        };
        let desired = matrix_for(settings);
        journal.pending = Some(desired);
        if let Err(error) = self.persist(&journal) {
            // An update failure must never discard the original from an active session.
            if !self.enabled {
                if let Err(cleanup) = self.release() {
                    return Err(format!("{error} {cleanup}"));
                }
            }
            return Err(error);
        }
        self.journal = Some(journal.clone());
        self.enabled = true; // A failed/partial write still requires restoration on normal close.
        let applied = self.api.write(desired).and_then(|_| {
            if matrices_match(self.api.read()?, desired) {
                Ok(())
            } else {
                Err("Windows aceptó aplicar, pero la lectura no coincide.".into())
            }
        });
        if let Err(error) = applied {
            return match self.reset() {
                Ok(()) => Err(format!("{error} Original restaurado/verificado.")),
                Err(restore) => Err(format!("{error} Restauración fallida: {restore}")),
            };
        }
        self.enabled = true;
        journal.applied = Some(desired);
        journal.pending = None;
        // The pre-write journal already contains the attempted matrix for crash recovery.
        if let Err(error) = self.persist(&journal) {
            self.recovery_required = true;
            self.error = Some(error.clone());
            return Err(error);
        }
        self.journal = Some(journal);
        self.error = None;
        Ok(())
    }
    fn reset(&mut self) -> Result<(), String> {
        match self.restore() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.recovery_required = true;
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    fn shutdown(&mut self) -> Result<(), String> {
        if self.enabled {
            self.reset()
        } else {
            self.release()
        }
    }
}
#[cfg(test)]
#[path = "windows_engine_tests.rs"]
mod tests;
