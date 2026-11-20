mod color_engine;
mod commands;
mod consent_audit;
mod diagnostic;
mod durable;
mod models;
mod single_instance;
pub mod watchdog;

use color_engine::{ColorEngine, ColorEngineManager};
use models::ProfileConfig;
use std::{fs, path::PathBuf, sync::Mutex};
#[cfg(feature = "wry")]
use tauri::{Manager, RunEvent};

pub(crate) fn replace_file(
    temporary: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain([0]).collect();
        let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(temporary.as_ptr()),
                windows::core::PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| std::io::Error::other(error.to_string()))
    }

    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(temporary, destination)
    }
}

pub struct ProfileService {
    path: PathBuf,
    config: Mutex<ProfileConfig>,
}

#[cfg(feature = "wry")]
fn shutdown_services(app: &tauri::AppHandle) -> Result<(), String> {
    let engine = app.state::<Mutex<ColorEngineManager>>();
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado durante el cierre.")?;
    let diagnostic = app.state::<Mutex<diagnostic::DiagnosticService>>();
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    if diagnostic.active() {
        diagnostic.restore_watchdog()?;
    }
    engine.shutdown()
}

impl ProfileService {
    fn open(path: PathBuf) -> Result<Self, String> {
        let config = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|error| format!("No se pudo leer la configuración: {error}"))?;
            let config: ProfileConfig = serde_json::from_str(&content)
                .map_err(|error| format!("JSON de configuración inválido: {error}"))?;
            config.validate()?;
            config
        } else {
            ProfileConfig::defaults()
        };
        let service = Self {
            path,
            config: Mutex::new(config),
        };
        if !service.path.exists() {
            let config = service
                .config
                .lock()
                .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
            service.save(&config)?;
        }
        Ok(service)
    }

    fn save(&self, config: &ProfileConfig) -> Result<(), String> {
        config.validate()?;
        crate::durable::json(&self.path, config)
            .map_err(|error| format!("No se pudo guardar la configuración: {error}"))
    }
}

#[cfg(any(feature = "wry", all(test, feature = "tauri-dispatch-tests")))]
macro_rules! app_invoke_handler {
    () => {
        tauri::generate_handler![
            commands::load_config,
            commands::get_diagnostic,
            commands::prepare_diagnostic,
            commands::cancel_diagnostic,
            commands::handoff_diagnostic,
            commands::start_diagnostic,
            commands::set_backend_preference,
            commands::get_capabilities,
            commands::get_engine_status,
            commands::set_filters_enabled,
            commands::restore_colors,
            commands::save_profile_settings,
            commands::select_profile,
            commands::create_profile,
            commands::duplicate_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::export_profiles,
            commands::import_profiles
        ]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(feature = "wry")]
pub fn run() {
    // Acquire before Tauri setup, profiles or recovery files. Fail closed on OS errors.
    #[cfg(target_os = "windows")]
    let _instance_guard =
        match single_instance::InstanceGuard::acquire("Global\\com.vyra.desktop.ColorEngine") {
            Ok(Some(guard)) => guard,
            Ok(None) => return,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            fs::create_dir_all(&directory)?;
            let service = ProfileService::open(directory.join("profiles.json"))
                .map_err(std::io::Error::other)?;
            let preference = service
                .config
                .lock()
                .map_err(|_| std::io::Error::other("Bloqueo de perfiles envenenado."))?
                .backend_preference;
            app.manage(service);
            app.manage(Mutex::new(diagnostic::DiagnosticService::new(
                directory.join("diagnostic-events.jsonl"),
            )));
            app.manage(Mutex::new(ColorEngineManager::with_recovery_journal(
                preference,
                directory.join("color-recovery.json"),
            )));
            Ok(())
        })
        .invoke_handler(app_invoke_handler!())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let result = shutdown_services(window.app_handle());
                if let Err(error) = result {
                    api.prevent_close();
                    eprintln!("Cierre cancelado: {error}");
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error al iniciar VYRA")
        .run(|app, event| {
            if let RunEvent::ExitRequested { api, .. } = &event {
                let result = shutdown_services(app);
                if let Err(error) = result {
                    api.prevent_exit();
                    eprintln!("Salida cancelada: {error}");
                }
            }
            if matches!(event, RunEvent::Exit) {
                let engine = app.state::<Mutex<ColorEngineManager>>();
                if let Ok(mut engine) = engine.lock() {
                    if let Err(error) = engine.shutdown() {
                        eprintln!("Restauración al salir falló; diario conservado: {error}");
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn profile_storage_persists_and_reloads_json() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vyra-profile-test-{}-{stamp}.json",
            std::process::id()
        ));
        let service = ProfileService::open(path.clone()).unwrap();
        let mut config = ProfileConfig::defaults();
        config.profiles[0].settings.saturation = 37;
        config.backend_preference = models::BackendPreference::Amd;
        service.save(&config).unwrap();
        drop(service);

        let reopened = ProfileService::open(path.clone()).unwrap();
        let restored = reopened.config.lock().unwrap();
        assert_eq!(restored.profiles[0].settings.saturation, 37);
        assert_eq!(restored.backend_preference, models::BackendPreference::Amd);
        drop(restored);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_profile_commit_preserves_destination_and_synced_temporary_evidence() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "vyra-profile-commit-failure-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("blocked.json");
        std::fs::create_dir(&path).unwrap();
        let temporary = path.with_extension("json.tmp");
        let config = ProfileConfig::defaults();
        let service = ProfileService {
            path: path.clone(),
            config: Mutex::new(config.clone()),
        };

        assert!(service.save(&config).is_err());
        assert!(path.is_dir(), "failed commit must preserve the destination");
        let staged = std::fs::read(&temporary).expect("synced temporary should remain for review");
        let decoded: ProfileConfig = serde_json::from_slice(&staged).unwrap();
        decoded.validate().unwrap();

        std::fs::remove_file(temporary).unwrap();
        std::fs::remove_dir(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

#[cfg(all(test, feature = "tauri-dispatch-tests"))]
mod tauri_dispatch_tests;
