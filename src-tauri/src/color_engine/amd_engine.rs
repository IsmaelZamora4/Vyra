use super::{Capabilities, ColorEngine, EngineStatus};
use crate::models::Settings;

const LIMITATION: &str = "AMD ADLX documenta controles de color por pantalla, pero VYRA aún no incluye el SDK/runtime nativo ADLX ni un adaptador probado. No se simula soporte.";

#[derive(Default)]
pub struct AmdColorEngine;

impl ColorEngine for AmdColorEngine {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            engine_name: "AMD ADLX (no disponible)".into(),
            selected_backend: "amd".into(),
            backend_preference: crate::models::BackendPreference::Amd,
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
            description: "AMD ADLX no disponible; pantalla sin cambios".into(),
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
