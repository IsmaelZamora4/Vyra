mod amd_engine;
mod display_inventory;
pub(crate) mod magnification;
mod matrix;
#[cfg(test)]
mod mock_engine;
mod nvidia_engine;
mod windows_engine;

use crate::models::Settings;
use serde::Serialize;
use std::path::PathBuf;

pub use amd_engine::AmdColorEngine;
pub use display_inventory::{DisplayAdapter, enumerate_display_adapters};
#[cfg(test)]
pub use mock_engine::MockColorEngine;
pub use nvidia_engine::NvidiaColorEngine;
pub use windows_engine::WindowsColorEngine;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub engine_name: String,
    pub selected_backend: String,
    pub backend_preference: crate::models::BackendPreference,
    pub is_simulation: bool,
    pub available: bool,
    pub supported_controls: Vec<String>,
    pub unsupported_controls: Vec<String>,
    pub per_monitor: bool,
    pub can_restore: bool,
    pub detected_adapters: Vec<DisplayAdapter>,
    pub limitation: String,
    pub activation_blocked_reason: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub enabled: bool,
    pub recovery_required: bool,
    pub description: String,
}

pub trait ColorEngine: Send {
    fn capabilities(&self) -> Capabilities;
    fn initialize(&mut self) -> Result<(), String>;
    fn get_current_state(&self) -> EngineStatus;
    fn apply(&mut self, settings: &Settings) -> Result<(), String>;
    fn reset(&mut self) -> Result<(), String>;
    fn shutdown(&mut self) -> Result<(), String>;
    fn diagnostic_read_current(&mut self) -> Result<magnification::Matrix, String> {
        Err("Este backend no proporciona lectura de matriz de diagnóstico.".into())
    }
}

pub struct ColorEngineManager {
    preference: crate::models::BackendPreference,
    windows: Box<dyn ColorEngine>,
    nvidia: NvidiaColorEngine,
    amd: AmdColorEngine,
}

#[derive(Clone, Copy)]
enum EngineBackendKind {
    Windows,
    Nvidia,
    Amd,
}

impl ColorEngineManager {
    #[cfg(test)]
    pub fn new(preference: crate::models::BackendPreference) -> Self {
        Self {
            preference,
            windows: Box::new(MockColorEngine::new()),
            nvidia: NvidiaColorEngine,
            amd: AmdColorEngine,
        }
    }

    pub fn with_recovery_journal(
        preference: crate::models::BackendPreference,
        recovery_file: PathBuf,
    ) -> Self {
        Self::build(preference, Some(recovery_file))
    }

    fn build(preference: crate::models::BackendPreference, recovery_file: Option<PathBuf>) -> Self {
        Self {
            preference,
            windows: Box::new(WindowsColorEngine::new(recovery_file)),
            nvidia: NvidiaColorEngine,
            amd: AmdColorEngine,
        }
    }

    pub fn set_preference(
        &mut self,
        preference: crate::models::BackendPreference,
    ) -> Result<(), String> {
        let state = self.get_current_state();
        if state.enabled || state.recovery_required {
            return Err("Desactiva el motor antes de cambiar el backend.".into());
        }
        self.preference = preference;
        Ok(())
    }

    pub fn diagnostic_preflight(&mut self) -> crate::diagnostic::DiagnosticPreflight {
        let state = self.windows.get_current_state();
        if state.recovery_required {
            return crate::diagnostic::DiagnosticPreflight::blocked(
                crate::diagnostic::DiagnosticBlocker::RecoveryPending,
                "Primero resuelve la recuperación pendiente; se conserva su diario.",
            );
        }
        if state.enabled {
            return crate::diagnostic::DiagnosticPreflight::blocked(
                crate::diagnostic::DiagnosticBlocker::FiltersActive,
                "Desactiva los filtros antes del diagnóstico.",
            );
        }
        if self.preference != crate::models::BackendPreference::Windows {
            return crate::diagnostic::DiagnosticPreflight::blocked(
                crate::diagnostic::DiagnosticBlocker::BackendMismatch,
                "Selecciona Windows experimental antes de preparar la prueba.",
            );
        }
        crate::diagnostic::DiagnosticPreflight::from_matrix(self.windows.diagnostic_read_current())
    }

    fn resolve_backend(&self) -> EngineBackendKind {
        use crate::models::BackendPreference;
        match self.preference {
            BackendPreference::Nvidia => EngineBackendKind::Nvidia,
            BackendPreference::Amd => EngineBackendKind::Amd,
            BackendPreference::Windows => EngineBackendKind::Windows,
            BackendPreference::Automatic => {
                // Automatic mode selects only a backend whose concrete color API is available.
                if self.nvidia.capabilities().available {
                    EngineBackendKind::Nvidia
                } else if self.amd.capabilities().available {
                    EngineBackendKind::Amd
                } else {
                    EngineBackendKind::Windows
                }
            }
        }
    }

    fn selected_backend(&self) -> &dyn ColorEngine {
        match self.resolve_backend() {
            EngineBackendKind::Windows => self.windows.as_ref(),
            EngineBackendKind::Nvidia => &self.nvidia,
            EngineBackendKind::Amd => &self.amd,
        }
    }

    fn selected_backend_mut(&mut self) -> &mut dyn ColorEngine {
        let backend = self.resolve_backend();
        match backend {
            EngineBackendKind::Windows => self.windows.as_mut(),
            EngineBackendKind::Nvidia => &mut self.nvidia,
            EngineBackendKind::Amd => &mut self.amd,
        }
    }
}

impl ColorEngine for ColorEngineManager {
    fn capabilities(&self) -> Capabilities {
        let mut capabilities = self.selected_backend().capabilities();
        capabilities.backend_preference = self.preference;
        capabilities.detected_adapters = enumerate_display_adapters();
        if self.preference == crate::models::BackendPreference::Automatic {
            capabilities.selected_backend = if capabilities.engine_name.starts_with("NVIDIA") {
                "nvidia"
            } else if capabilities.engine_name.starts_with("AMD") {
                "amd"
            } else {
                "windows"
            }
            .into();
        }
        if capabilities.engine_name.starts_with("NVIDIA")
            && !capabilities
                .detected_adapters
                .iter()
                .any(|adapter| adapter.vendor == "NVIDIA")
        {
            capabilities.limitation = "No se detecta un adaptador NVIDIA en los dispositivos de pantalla de Windows. Además, NVAPI público no documenta Digital Vibrance Control.".into();
        }
        if capabilities.engine_name.starts_with("AMD")
            && !capabilities
                .detected_adapters
                .iter()
                .any(|adapter| adapter.vendor == "AMD")
        {
            capabilities.limitation = "No se detecta un adaptador AMD en los dispositivos de pantalla de Windows. ADLX está documentada, pero el adaptador nativo aún no está integrado.".into();
        }
        capabilities
    }

    fn initialize(&mut self) -> Result<(), String> {
        let capabilities = self.capabilities();
        if !capabilities.available {
            return Err(capabilities.limitation);
        }
        self.selected_backend_mut().initialize()
    }

    fn get_current_state(&self) -> EngineStatus {
        let windows_state = self.windows.get_current_state();
        if windows_state.enabled || windows_state.recovery_required {
            return windows_state;
        }
        let capabilities = self.capabilities();
        if !capabilities.available {
            return EngineStatus {
                enabled: false,
                recovery_required: false,
                description: format!("Backend no disponible: {}", capabilities.engine_name),
            };
        }
        self.selected_backend().get_current_state()
    }

    fn apply(&mut self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        if self.windows.get_current_state().recovery_required {
            return Err(
                "Hay una restauración pendiente. Restaura el estado anterior antes de aplicar."
                    .into(),
            );
        }
        if matches!(self.resolve_backend(), EngineBackendKind::Windows) {
            crate::diagnostic::require_independent_recovery()?;
        }
        let capabilities = self.capabilities();
        if !capabilities.available {
            return Err(capabilities.limitation);
        }
        self.selected_backend_mut().apply(settings)
    }

    fn reset(&mut self) -> Result<(), String> {
        let state = self.windows.get_current_state();
        if state.enabled || state.recovery_required {
            return self.windows.reset();
        }
        if self.capabilities().available {
            self.selected_backend_mut().reset()
        } else {
            Ok(())
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        // Also retry cleanup after an unsuccessful read-only probe. An idle pending
        // journal is never restored automatically by WindowsColorEngine::shutdown.
        self.windows.shutdown()?;
        match self.resolve_backend() {
            EngineBackendKind::Windows => Ok(()),
            EngineBackendKind::Nvidia => self.nvidia.shutdown(),
            EngineBackendKind::Amd => self.amd.shutdown(),
        }
    }
}

pub fn matrix_for(settings: &Settings) -> [[f32; 5]; 5] {
    matrix::matrix_for(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_settings_produce_identity_matrix() {
        let matrix = matrix_for(&Settings::default());
        let identity = [
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(matrix, identity);
    }

    #[test]
    fn mock_engine_never_claims_to_change_the_display() {
        let mut engine = MockColorEngine::new();
        assert!(engine.capabilities().is_simulation);
        engine.apply(&Settings::default()).unwrap();
        assert_eq!(engine.current_matrix(), matrix_for(&Settings::default()));
        assert!(
            engine
                .get_current_state()
                .description
                .contains("pantalla sin cambios")
        );
        engine.reset().unwrap();
        assert!(!engine.get_current_state().enabled);
    }

    #[test]
    fn mock_engine_keeps_state_unchanged_when_apply_fails() {
        let mut engine = MockColorEngine::with_failures(true, false);
        let before = engine.current_matrix();
        assert_eq!(
            engine.apply(&Settings::default()).unwrap_err(),
            "Fallo simulado al aplicar."
        );
        assert!(!engine.get_current_state().enabled);
        assert_eq!(engine.current_matrix(), before);
    }

    #[test]
    fn mock_engine_keeps_active_state_when_restore_fails() {
        let mut engine = MockColorEngine::with_failures(false, true);
        let mut settings = Settings::default();
        settings.saturation = 25;
        engine.apply(&settings).unwrap();
        let applied = engine.current_matrix();
        assert_eq!(engine.reset().unwrap_err(), "Fallo simulado al restaurar.");
        assert!(engine.get_current_state().enabled);
        assert_eq!(engine.current_matrix(), applied);
    }

    #[test]
    fn unavailable_vendor_backend_does_not_claim_display_controls() {
        let mut engine = ColorEngineManager::new(crate::models::BackendPreference::Nvidia);
        let capabilities = engine.capabilities();
        assert!(!capabilities.available);
        assert!(capabilities.supported_controls.is_empty());
        assert!(engine.apply(&Settings::default()).is_err());
        assert!(!engine.get_current_state().enabled);
    }

    #[test]
    fn backend_change_and_preflight_are_blocked_while_active() {
        use crate::models::BackendPreference;
        let mut engine = ColorEngineManager::new(BackendPreference::Windows);
        // Simulate an already-active session without bypassing the production activation gate.
        engine.windows.apply(&Settings::default()).unwrap();
        assert!(engine.set_preference(BackendPreference::Nvidia).is_err());
        assert_eq!(engine.preference, BackendPreference::Windows);
        assert!(engine.get_current_state().enabled);
        engine.reset().unwrap();
        engine.reset().unwrap();
        assert!(!engine.get_current_state().enabled);
        engine.set_preference(BackendPreference::Amd).unwrap();
    }

    #[test]
    fn ordinary_activation_cannot_bypass_independent_recovery_gate() {
        let mut engine = ColorEngineManager::new(crate::models::BackendPreference::Windows);
        assert!(
            engine
                .apply(&Settings::default())
                .unwrap_err()
                .contains("watchdog")
        );
        assert!(!engine.windows.get_current_state().enabled);
    }

    #[test]
    fn inconsistent_backend_selection_cannot_hide_active_windows_state() {
        let mut engine = ColorEngineManager::new(crate::models::BackendPreference::Amd);
        engine.windows.apply(&Settings::default()).unwrap();
        assert!(engine.get_current_state().enabled);
        assert!(
            engine
                .set_preference(crate::models::BackendPreference::Nvidia)
                .is_err()
        );
        engine.reset().unwrap();
        assert!(!engine.windows.get_current_state().enabled);
    }
}
