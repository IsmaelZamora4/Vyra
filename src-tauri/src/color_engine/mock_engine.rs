use super::{Capabilities, ColorEngine, EngineStatus, matrix_for};
use crate::models::Settings;

pub struct MockColorEngine {
    enabled: bool,
    matrix: [[f32; 5]; 5],
    #[cfg(test)]
    fail_apply: bool,
    #[cfg(test)]
    fail_reset: bool,
}

impl MockColorEngine {
    pub fn new() -> Self {
        Self {
            enabled: false,
            matrix: matrix_for(&Settings::default()),
            #[cfg(test)]
            fail_apply: false,
            #[cfg(test)]
            fail_reset: false,
        }
    }

    #[cfg(test)]
    pub fn with_failures(fail_apply: bool, fail_reset: bool) -> Self {
        Self {
            fail_apply,
            fail_reset,
            ..Self::new()
        }
    }

    pub fn current_matrix(&self) -> [[f32; 5]; 5] {
        self.matrix
    }
}

impl ColorEngine for MockColorEngine {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            engine_name: "Simulación de desarrollo/pruebas".into(),
            selected_backend: "mock".into(),
            backend_preference: crate::models::BackendPreference::Automatic,
            is_simulation: true,
            available: true,
            supported_controls: vec![
                "brightness".into(),
                "contrast".into(),
                "saturation".into(),
                "hue".into(),
            ],
            unsupported_controls: vec!["gamma".into(), "sharpness".into()],
            per_monitor: false,
            can_restore: true,
            detected_adapters: Vec::new(),
            limitation: "Solo simula estado en memoria. No modifica la pantalla.".into(),
            activation_blocked_reason: None,
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn get_current_state(&self) -> EngineStatus {
        EngineStatus {
            enabled: self.enabled,
            recovery_required: false,
            description: if self.enabled {
                "Simulación activa; pantalla sin cambios"
            } else {
                "Simulación apagada; pantalla sin cambios"
            }
            .into(),
        }
    }

    fn apply(&mut self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        #[cfg(test)]
        if self.fail_apply {
            return Err("Fallo simulado al aplicar.".into());
        }
        self.initialize()?;
        self.matrix = matrix_for(settings);
        self.enabled = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        #[cfg(test)]
        if self.fail_reset {
            return Err("Fallo simulado al restaurar.".into());
        }
        self.matrix = matrix_for(&Settings::default());
        self.enabled = false;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.reset()
    }

    fn diagnostic_read_current(&mut self) -> Result<[[f32; 5]; 5], String> {
        Ok(self.matrix)
    }
}
