use super::{Capabilities, ColorEngine, EngineStatus};
use crate::models::Settings;

const LIMITATION: &str = "NVIDIA está detectada por Windows, pero el SDK NVAPI público actual no expone Digital Vibrance Control. No se invocan funciones privadas o no documentadas; este backend no puede modificar color.";

#[derive(Default)]
pub struct NvidiaColorEngine;

impl ColorEngine for NvidiaColorEngine {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            engine_name: "NVIDIA NVAPI (no disponible)".into(),
            selected_backend: "nvidia".into(),
            backend_preference: crate::models::BackendPreference::Nvidia,
            is_simulation: false,
            available: false,
            supported_controls: Vec::new(),
            unsupported_controls: vec![
                "saturation".into(),
                "brightness".into(),
                "contrast".into(),
                "hue".into(),
                "gamma".into(),
                "sharpness".into(),
            ],
            per_monitor: false,
            can_restore: false,
            detected_adapters: Vec::new(),
            limitation: LIMITATION.into(),
            activation_blocked_reason: Some(LIMITATION.into()),
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        Err(LIMITATION.into())
    }

    fn get_current_state(&self) -> EngineStatus {
        EngineStatus {
            enabled: false,
            recovery_required: false,
            description: "NVIDIA NVAPI no disponible; pantalla sin cambios".into(),
        }
    }

    fn apply(&mut self, _settings: &Settings) -> Result<(), String> {
        Err(LIMITATION.into())
    }

    fn reset(&mut self) -> Result<(), String> {
        Err(LIMITATION.into())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}
