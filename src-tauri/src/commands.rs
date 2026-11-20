use crate::{
    ProfileService,
    color_engine::{Capabilities, ColorEngine, ColorEngineManager, EngineStatus},
    models::{
        BackendPreference, MAX_PROFILES, Profile, ProfileConfig, ProfileExport, SCHEMA_VERSION,
        Settings, validate_profile,
    },
};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

fn confirm_diagnostic_start<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<
    (bool, crate::consent_audit::NativeConfirmationTrace),
    (
        String,
        Option<crate::consent_audit::NativeConfirmationTrace>,
    ),
> {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            Win32::UI::WindowsAndMessaging::{
                IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MB_YESNO, MessageBoxW,
            },
            core::PCWSTR,
        };
        let owner = window.hwnd().map_err(|error| {
            (
                format!("No se pudo obtener la ventana de confirmación: {error}"),
                None,
            )
        })?;
        let title: Vec<u16> = "VYRA — Confirmar ensayo visual"
            .encode_utf16()
            .chain([0])
            .collect();
        let message: Vec<u16> = "VYRA aplicará saturación global fija +5 durante un máximo de 10 segundos. El watchdog intentará restaurar la matriz original, pero Windows puede bloquear una llamada síncrona y no se garantiza recuperación infalible. Cierra otras aplicaciones que puedan cambiar Magnification. ¿Confirmas aplicar ahora?"
            .encode_utf16()
            .chain([0])
            .collect();
        let result = unsafe {
            MessageBoxW(
                Some(owner),
                PCWSTR(message.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND,
            )
        };
        let classification = if result.0 == 0 {
            "apiError"
        } else if result == IDYES {
            "yes"
        } else {
            "noOrClosed"
        };
        let trace = crate::consent_audit::NativeConfirmationTrace {
            raw_return_code: Some(result.0),
            classification: classification.into(),
        };
        if result.0 == 0 {
            Err((
                "Windows no pudo mostrar la confirmación del ensayo.".into(),
                Some(trace),
            ))
        } else {
            Ok((result == IDYES, trace))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Ok((
            false,
            crate::consent_audit::NativeConfirmationTrace {
                raw_return_code: None,
                classification: "unsupportedPlatform".into(),
            },
        ))
    }
}

#[tauri::command]
pub fn get_diagnostic(
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<crate::diagnostic::DiagnosticReport, String> {
    Ok(diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?
        .report())
}

#[tauri::command]
pub fn prepare_diagnostic(
    engine: State<'_, Mutex<ColorEngineManager>>,
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<crate::diagnostic::DiagnosticReport, String> {
    let mut engine = engine.lock().map_err(|_| "Bloqueo del motor envenenado.")?;
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    let previous = format!("{:?}", diagnostic.report().phase);
    diagnostic.audit_command(
        "prepare_diagnostic",
        "requested",
        &previous,
        &previous,
        "Solicitud recibida",
        None,
    )?;
    if let Err(error) = diagnostic.ensure_idle() {
        let next = format!("{:?}", diagnostic.report().phase);
        let _ = diagnostic.audit_command(
            "prepare_diagnostic",
            "rejected",
            &previous,
            &next,
            "active_session_or_recovery_pending",
            None,
        );
        return Err(error);
    }
    let preflight = engine.diagnostic_preflight();
    let result = diagnostic.prepare(preflight).and_then(|report| {
        if report.current_state_read_accepted == Some(true) {
            diagnostic.prepare_watchdog()
        } else {
            Ok(report)
        }
    });
    match result {
        Ok(report) => {
            diagnostic.audit_command(
                "prepare_diagnostic",
                "completed",
                &previous,
                &format!("{:?}", report.phase),
                &report.reason,
                None,
            )?;
            Ok(report)
        }
        Err(error) => {
            let next = format!("{:?}", diagnostic.report().phase);
            let _ = diagnostic.audit_command(
                "prepare_diagnostic",
                "rejected",
                &previous,
                &next,
                &error,
                None,
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub fn cancel_diagnostic(
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<crate::diagnostic::DiagnosticReport, String> {
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    let previous = format!("{:?}", diagnostic.report().phase);
    let _ = diagnostic.audit_command(
        "cancel_diagnostic",
        "requested",
        &previous,
        &previous,
        "Solicitud recibida",
        None,
    );
    let result = diagnostic.cancel();
    match result {
        Ok(report) => {
            let _ = diagnostic.audit_command(
                "cancel_diagnostic",
                "completed",
                &previous,
                &format!("{:?}", report.phase),
                &report.reason,
                None,
            );
            Ok(report)
        }
        Err(error) => {
            let next = format!("{:?}", diagnostic.report().phase);
            let _ = diagnostic.audit_command(
                "cancel_diagnostic",
                "failed",
                &previous,
                &next,
                &error,
                None,
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub fn handoff_diagnostic(
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<crate::diagnostic::DiagnosticReport, String> {
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    let previous = format!("{:?}", diagnostic.report().phase);
    let _ = diagnostic.audit_command(
        "handoff_diagnostic",
        "requested",
        &previous,
        &previous,
        "Traspaso a operación manual solicitado",
        None,
    );
    let mut report = diagnostic.cancel()?;
    report.reason = "Preflight cancelado sin Apply. Cierra la sesión de automatización y toma control manual antes de cualquier ensayo futuro; VYRA no puede verificar ese traspaso.".into();
    diagnostic.audit_command(
        "handoff_diagnostic",
        "completed",
        &previous,
        "Cancelled",
        &report.reason,
        None,
    )?;
    Ok(report)
}

/// Authorization that must pass before the native confirmation window is reachable.
fn authorize_diagnostic_start(
    diagnostic: &mut crate::diagnostic::DiagnosticService,
    attempt_id: &str,
) -> Result<String, String> {
    let previous = format!("{:?}", diagnostic.report().phase);
    diagnostic.audit_command(
        "start_diagnostic",
        "requested",
        &previous,
        &previous,
        "Solicitud recibida",
        None,
    )?;
    if !crate::watchdog::REAL_DIAGNOSTIC_ENABLED {
        diagnostic.record_incident_lockdown()?;
        let reason = "Ensayo visual bloqueado por incidente de consentimiento no acreditado.";
        diagnostic.audit_command(
            "start_diagnostic",
            "rejected",
            &previous,
            "ConsentIncidentLockdown",
            reason,
            None,
        )?;
        return Err(reason.into());
    }
    if let Err(error) = diagnostic.claim_attempt(attempt_id) {
        let next = format!("{:?}", diagnostic.report().phase);
        let _ = diagnostic.audit_command(
            "start_diagnostic",
            "rejected",
            &previous,
            &next,
            &error,
            None,
        );
        return Err(error);
    }
    if !diagnostic.report().can_request_confirmation {
        let reason = "El watchdog no está armado para pedir confirmación.";
        let _ = diagnostic.audit_command(
            "start_diagnostic",
            "rejected",
            &previous,
            "ConsentIncidentLockdown",
            "preflight_gate_closed",
            None,
        );
        return Err(reason.into());
    }
    if let Err(error) = diagnostic.begin_confirmation() {
        let _ = diagnostic.audit_command(
            "start_diagnostic",
            "rejected",
            &previous,
            "ConsentIncidentLockdown",
            "confirmation_gate_rejected",
            None,
        );
        return Err(error);
    }
    Ok(previous)
}

trait DiagnosticStartPort {
    fn authorize(&mut self, attempt_id: &str) -> Result<String, String>;
    fn confirm(
        &mut self,
    ) -> Result<
        (bool, crate::consent_audit::NativeConfirmationTrace),
        (
            String,
            Option<crate::consent_audit::NativeConfirmationTrace>,
        ),
    >;
    fn recheck(&mut self) -> crate::diagnostic::DiagnosticPreflight;
    fn audit(
        &mut self,
        outcome: &str,
        previous: &str,
        next: &str,
        reason: &str,
        native: Option<crate::consent_audit::NativeConfirmationTrace>,
    ) -> Result<(), String>;
    fn validate(&mut self, recheck: crate::diagnostic::DiagnosticPreflight) -> Result<(), String>;
    fn confirmation_failed(&mut self, reason: &str);
    fn confirmation_declined(&mut self);
    fn arm(&mut self) -> Result<(), String>;
    fn request_apply(&mut self) -> Result<(), crate::watchdog::client::ApplyRequestError>;
    fn cancel_before_apply(&mut self);
    fn restore_uncertain(&mut self);
    fn mark_uncertain(&mut self, reason: &str);
}

/// The Tauri handler and deterministic integration tests share this exact transaction order.
/// Every durable transition before Apply must succeed before the next side effect is requested.
fn run_diagnostic_start(
    port: &mut impl DiagnosticStartPort,
    attempt_id: &str,
) -> Result<bool, String> {
    let previous = match port.authorize(attempt_id) {
        Ok(previous) => previous,
        Err(error) => {
            port.cancel_before_apply();
            return Err(error);
        }
    };
    let (confirmed, native_trace) = match port.confirm() {
        Ok(result) => result,
        Err((error, trace)) => {
            port.confirmation_failed(&error);
            let _ = port.audit(
                "confirmationError",
                &previous,
                "WatchdogError",
                &error,
                trace,
            );
            return Err(error);
        }
    };
    if let Err(error) = port.audit(
        if confirmed {
            "confirmationAccepted"
        } else {
            "confirmationDeclined"
        },
        &previous,
        if confirmed { "Confirming" } else { "Cancelled" },
        &native_trace.classification,
        Some(native_trace.clone()),
    ) {
        if confirmed {
            port.cancel_before_apply();
        } else {
            port.confirmation_declined();
        }
        return Err(format!(
            "No se persistió el resultado de confirmación; no se solicitará Arm ni Apply: {error}"
        ));
    }
    if !confirmed {
        port.confirmation_declined();
        return Ok(false);
    }

    let recheck = port.recheck();
    if let Err(error) = port.validate(recheck) {
        port.cancel_before_apply();
        let _ = port.audit(
            "authorizationRejected",
            &previous,
            "Failed",
            &error,
            Some(native_trace),
        );
        return Err(error);
    }

    if let Err(error) = port.audit(
        "authorizationRecorded",
        &previous,
        "Authorized",
        "Confirmación y recheck aceptados; Apply aún no solicitado",
        Some(native_trace.clone()),
    ) {
        port.cancel_before_apply();
        return Err(format!(
            "No se persistió la autorización; no se solicitó Apply: {error}"
        ));
    }
    if let Err(error) = port.audit(
        "armRequested",
        "Authorized",
        "Arming",
        "Persistir antes de solicitar Arm",
        Some(native_trace.clone()),
    ) {
        port.cancel_before_apply();
        return Err(format!(
            "No se persistió armRequested; no se solicitó Arm ni Apply: {error}"
        ));
    }
    if let Err(error) = port.arm() {
        port.cancel_before_apply();
        let _ = port.audit(
            "armRejected",
            "Arming",
            "Failed",
            &error,
            Some(native_trace),
        );
        return Err(error);
    }
    if let Err(error) = port.audit(
        "armAccepted",
        "Arming",
        "Armed",
        "Watchdog confirmó nonce nuevo y deadline",
        Some(native_trace.clone()),
    ) {
        port.cancel_before_apply();
        return Err(format!(
            "Arm aceptado, pero auditoría falló; Apply no solicitado: {error}"
        ));
    }
    if let Err(error) = port.audit(
        "applyRequested",
        "Armed",
        "ApplyPending",
        "Persistir antes de enviar Apply al watchdog",
        Some(native_trace.clone()),
    ) {
        port.cancel_before_apply();
        return Err(format!(
            "No se persistió applyRequested; Apply no solicitado: {error}"
        ));
    }

    match port.request_apply() {
        Ok(()) => {
            if let Err(error) = port.audit(
                "applyCommandSent",
                "ApplyPending",
                "Applying",
                "IPC enviado; aceptación de Windows todavía pendiente",
                Some(native_trace),
            ) {
                port.mark_uncertain(&error);
                port.restore_uncertain();
                return Err(format!(
                    "Apply pudo haberse enviado; resultado incierto y recuperación prioritaria: {error}"
                ));
            }
            Ok(true)
        }
        Err(error) if error.may_have_been_sent() => {
            let reason = error.reason().to_owned();
            port.mark_uncertain(&reason);
            port.restore_uncertain();
            Err(format!(
                "Apply pudo haberse enviado; resultado incierto y recuperación prioritaria: {reason}"
            ))
        }
        Err(error) => {
            let reason = error.reason().to_owned();
            port.cancel_before_apply();
            let _ = port.audit(
                "applyNotSent",
                "ApplyPending",
                "Failed",
                &reason,
                Some(native_trace),
            );
            Err(reason)
        }
    }
}

struct TauriDiagnosticStartPort<'a, F>
where
    F: FnMut() -> Result<
        (bool, crate::consent_audit::NativeConfirmationTrace),
        (
            String,
            Option<crate::consent_audit::NativeConfirmationTrace>,
        ),
    >,
{
    engine: &'a mut ColorEngineManager,
    diagnostic: &'a mut crate::diagnostic::DiagnosticService,
    confirm: F,
}
impl<F> DiagnosticStartPort for TauriDiagnosticStartPort<'_, F>
where
    F: FnMut() -> Result<
        (bool, crate::consent_audit::NativeConfirmationTrace),
        (
            String,
            Option<crate::consent_audit::NativeConfirmationTrace>,
        ),
    >,
{
    fn authorize(&mut self, attempt_id: &str) -> Result<String, String> {
        authorize_diagnostic_start(self.diagnostic, attempt_id)
    }
    fn confirm(
        &mut self,
    ) -> Result<
        (bool, crate::consent_audit::NativeConfirmationTrace),
        (
            String,
            Option<crate::consent_audit::NativeConfirmationTrace>,
        ),
    > {
        (self.confirm)()
    }
    fn recheck(&mut self) -> crate::diagnostic::DiagnosticPreflight {
        self.engine.diagnostic_preflight()
    }
    fn audit(
        &mut self,
        outcome: &str,
        previous: &str,
        next: &str,
        reason: &str,
        native: Option<crate::consent_audit::NativeConfirmationTrace>,
    ) -> Result<(), String> {
        self.diagnostic
            .audit_command("start_diagnostic", outcome, previous, next, reason, native)
    }
    fn validate(&mut self, recheck: crate::diagnostic::DiagnosticPreflight) -> Result<(), String> {
        self.diagnostic.validate_after_native_confirmation(recheck)
    }
    fn confirmation_failed(&mut self, reason: &str) {
        let _ = self.diagnostic.fail_confirmation(reason);
    }
    fn confirmation_declined(&mut self) {
        let _ = self.diagnostic.reject_confirmation();
    }
    fn arm(&mut self) -> Result<(), String> {
        self.diagnostic.arm_after_native_confirmation()
    }
    fn request_apply(&mut self) -> Result<(), crate::watchdog::client::ApplyRequestError> {
        self.diagnostic.request_diagnostic_apply()
    }
    fn cancel_before_apply(&mut self) {
        let _ = self.diagnostic.cancel();
    }
    fn restore_uncertain(&mut self) {
        let _ = self.diagnostic.restore_watchdog();
    }
    fn mark_uncertain(&mut self, reason: &str) {
        self.diagnostic.mark_apply_outcome_uncertain(reason);
    }
}

#[tauri::command]
pub fn start_diagnostic<R: tauri::Runtime>(
    attempt_id: String,
    window: tauri::WebviewWindow<R>,
    engine: State<'_, Mutex<ColorEngineManager>>,
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<bool, String> {
    let mut engine = engine.lock().map_err(|_| "Bloqueo del motor envenenado.")?;
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    let mut port = TauriDiagnosticStartPort {
        engine: &mut engine,
        diagnostic: &mut diagnostic,
        confirm: || confirm_diagnostic_start(&window),
    };
    run_diagnostic_start(&mut port, &attempt_id)
}

fn profile_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("custom-{now}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[tauri::command]
pub fn load_config(service: State<'_, ProfileService>) -> Result<ProfileConfig, String> {
    service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())
        .map(|config| config.clone())
}

#[tauri::command]
pub fn set_backend_preference(
    preference: BackendPreference,
    service: State<'_, ProfileService>,
    engine: State<'_, Mutex<ColorEngineManager>>,
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let old_config = config.clone();
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?
        .ensure_idle()?;
    engine.set_preference(preference)?;
    config.backend_preference = preference;
    if let Err(error) = service.save(&config) {
        *config = old_config.clone();
        let _ = engine.set_preference(old_config.backend_preference);
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn get_capabilities(
    engine: State<'_, Mutex<ColorEngineManager>>,
) -> Result<Capabilities, String> {
    let engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    Ok(engine.capabilities())
}

#[tauri::command]
pub fn get_engine_status(
    engine: State<'_, Mutex<ColorEngineManager>>,
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<EngineStatus, String> {
    let engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    Ok(diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?
        .status_override()
        .unwrap_or_else(|| engine.get_current_state()))
}

#[tauri::command]
pub fn set_filters_enabled(
    enabled: bool,
    service: State<'_, ProfileService>,
    engine: State<'_, Mutex<ColorEngineManager>>,
) -> Result<EngineStatus, String> {
    let config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    if enabled {
        let profile = config
            .profiles
            .iter()
            .find(|profile| profile.id == config.selected_profile_id)
            .ok_or_else(|| "El perfil seleccionado no existe.".to_string())?;
        profile.settings.validate()?;
        engine.apply(&profile.settings)?;
    } else {
        engine.reset()?;
    }
    Ok(engine.get_current_state())
}

#[tauri::command]
pub fn restore_colors(
    engine: State<'_, Mutex<ColorEngineManager>>,
    diagnostic: State<'_, Mutex<crate::diagnostic::DiagnosticService>>,
) -> Result<EngineStatus, String> {
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    let mut diagnostic = diagnostic
        .lock()
        .map_err(|_| "Bloqueo del diagnóstico envenenado.")?;
    let previous = format!("{:?}", diagnostic.report().phase);
    let _ = diagnostic.audit_command(
        "restore_colors",
        "requested",
        &previous,
        &previous,
        "Restauración manual solicitada",
        None,
    );
    let result: Result<EngineStatus, String> = (|| {
        if diagnostic.needs_restoration() {
            diagnostic.restore_watchdog()?;
        }
        engine.reset()?;
        Ok(engine.get_current_state())
    })();
    match result {
        Ok(status) => {
            let _ = diagnostic.audit_command(
                "restore_colors",
                "completed",
                &previous,
                "Restored",
                "Restauración solicitada completada",
                None,
            );
            Ok(status)
        }
        Err(error) => {
            let _ = diagnostic.audit_command(
                "restore_colors",
                "failed",
                &previous,
                "Failed",
                &error,
                None,
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub fn save_profile_settings(
    profile_id: String,
    settings: Settings,
    service: State<'_, ProfileService>,
) -> Result<ProfileConfig, String> {
    settings.validate()?;
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let old_config = config.clone();
    let profile = config
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "El perfil no existe.".to_string())?;
    if profile.builtin {
        return Err("Duplica un preset antes de cambiar sus ajustes.".into());
    }
    profile.settings = settings;
    config.validate()?;
    if let Err(error) = service.save(&config) {
        *config = old_config.clone();
        return Err(error);
    }

    Ok(config.clone())
}

#[tauri::command]
pub fn select_profile(
    profile_id: String,
    service: State<'_, ProfileService>,
    engine: State<'_, Mutex<ColorEngineManager>>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let next_settings = config
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "El perfil no existe.".to_string())?
        .settings
        .clone();
    let old_config = config.clone();
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    if engine.get_current_state().enabled {
        engine.apply(&next_settings)?;
    }
    config.selected_profile_id = profile_id;
    if let Err(error) = service.save(&config) {
        *config = old_config.clone();
        if engine.get_current_state().enabled {
            if let Some(old) = old_config
                .profiles
                .iter()
                .find(|profile| profile.id == old_config.selected_profile_id)
            {
                let _ = engine.apply(&old.settings);
            }
        }
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn create_profile(
    name: String,
    service: State<'_, ProfileService>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    if config.profiles.len() >= MAX_PROFILES {
        return Err(format!("Límite de {MAX_PROFILES} perfiles alcanzado."));
    }
    let old_config = config.clone();
    let source = config
        .profiles
        .iter()
        .find(|profile| profile.id == config.selected_profile_id)
        .ok_or_else(|| "El perfil seleccionado no existe.".to_string())?
        .settings
        .clone();
    let profile = Profile {
        id: profile_id(),
        name: name.trim().to_string(),
        builtin: false,
        settings: source,
    };
    validate_profile(&profile)?;
    config.profiles.push(profile.clone());
    config.selected_profile_id = profile.id;
    if let Err(error) = service.save(&config) {
        *config = old_config;
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn duplicate_profile(
    source_id: String,
    name: String,
    service: State<'_, ProfileService>,
    engine: State<'_, Mutex<ColorEngineManager>>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    if config.profiles.len() >= MAX_PROFILES {
        return Err(format!("Límite de {MAX_PROFILES} perfiles alcanzado."));
    }
    let settings = config
        .profiles
        .iter()
        .find(|profile| profile.id == source_id)
        .ok_or_else(|| "El perfil no existe.".to_string())?
        .settings
        .clone();
    let profile = Profile {
        id: profile_id(),
        name: name.trim().to_string(),
        builtin: false,
        settings,
    };
    validate_profile(&profile)?;
    let old_config = config.clone();
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    if engine.get_current_state().enabled {
        engine.apply(&profile.settings)?;
    }
    config.profiles.push(profile.clone());
    config.selected_profile_id = profile.id;
    if let Err(error) = service.save(&config) {
        *config = old_config.clone();
        if engine.get_current_state().enabled {
            if let Some(old) = old_config
                .profiles
                .iter()
                .find(|item| item.id == old_config.selected_profile_id)
            {
                let _ = engine.apply(&old.settings);
            }
        }
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn rename_profile(
    profile_id: String,
    name: String,
    service: State<'_, ProfileService>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let old_config = config.clone();
    let profile = config
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "El perfil no existe.".to_string())?;
    if profile.builtin {
        return Err("Duplica un preset antes de cambiar su nombre.".into());
    }
    profile.name = name.trim().to_string();
    validate_profile(profile)?;
    if let Err(error) = service.save(&config) {
        *config = old_config;
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn delete_profile(
    profile_id: String,
    service: State<'_, ProfileService>,
    engine: State<'_, Mutex<ColorEngineManager>>,
) -> Result<ProfileConfig, String> {
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let profile = config
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "El perfil no existe.".to_string())?;
    if profile.builtin {
        return Err("Los presets originales no se pueden eliminar.".into());
    }
    let old_config = config.clone();
    let fallback = if config.selected_profile_id == profile_id {
        Some(
            config
                .profiles
                .iter()
                .find(|item| item.id != profile_id)
                .ok_or_else(|| "No queda un perfil para seleccionar.".to_string())?
                .clone(),
        )
    } else {
        None
    };
    let mut engine = engine
        .lock()
        .map_err(|_| "Bloqueo del motor envenenado.".to_string())?;
    if let Some(profile) = &fallback {
        if engine.get_current_state().enabled {
            engine.apply(&profile.settings)?;
        }
        config.selected_profile_id = profile.id.clone();
    }
    config.profiles.retain(|profile| profile.id != profile_id);
    if let Err(error) = service.save(&config) {
        *config = old_config.clone();
        if engine.get_current_state().enabled {
            if let Some(old) = old_config
                .profiles
                .iter()
                .find(|item| item.id == old_config.selected_profile_id)
            {
                let _ = engine.apply(&old.settings);
            }
        }
        return Err(error);
    }
    Ok(config.clone())
}

#[tauri::command]
pub fn export_profiles(service: State<'_, ProfileService>) -> Result<String, String> {
    let config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let export = ProfileExport {
        schema_version: SCHEMA_VERSION,
        profiles: config.profiles.clone(),
    };
    serde_json::to_string_pretty(&export).map_err(|error| format!("No se pudo exportar: {error}"))
}

#[tauri::command]
pub fn import_profiles(
    json: String,
    service: State<'_, ProfileService>,
) -> Result<ProfileConfig, String> {
    let mut incoming: ProfileExport =
        serde_json::from_str(&json).map_err(|error| format!("JSON inválido: {error}"))?;
    if incoming.schema_version != SCHEMA_VERSION {
        return Err("Versión JSON no compatible.".into());
    }
    if incoming.profiles.len() > MAX_PROFILES {
        return Err(format!(
            "El archivo supera el límite de {MAX_PROFILES} perfiles."
        ));
    }
    let mut config = service
        .config
        .lock()
        .map_err(|_| "Bloqueo de perfiles envenenado.".to_string())?;
    let old_config = config.clone();
    let customs = config
        .profiles
        .iter()
        .filter(|profile| !profile.builtin)
        .count();
    if customs
        + incoming.profiles.len()
        + config
            .profiles
            .iter()
            .filter(|profile| profile.builtin)
            .count()
        > MAX_PROFILES
    {
        return Err(format!("Límite de {MAX_PROFILES} perfiles alcanzado."));
    }
    for profile in &mut incoming.profiles {
        profile.id = profile_id();
        profile.builtin = false;
        validate_profile(profile)?;
    }
    config.profiles.extend(incoming.profiles);
    if let Err(error) = service.save(&config) {
        *config = old_config;
        return Err(error);
    }
    Ok(config.clone())
}

#[cfg(test)]
mod authorization_tests {
    use super::*;
    use crate::diagnostic::DiagnosticService;
    use crate::watchdog::client::ApplyRequestError;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FakeStartPort {
        gate: bool,
        valid_attempt: bool,
        consumed: bool,
        confirmed: bool,
        validation_ok: bool,
        arm_ok: bool,
        apply_error: Option<ApplyRequestError>,
        fail_audit_at: Option<&'static str>,
        events: Vec<&'static str>,
        confirmations: usize,
        arms: usize,
        applies: usize,
        cancels: usize,
        uncertain: bool,
        confirmation_gate: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
    }
    impl Default for FakeStartPort {
        fn default() -> Self {
            Self {
                gate: true,
                valid_attempt: true,
                consumed: false,
                confirmed: true,
                validation_ok: true,
                arm_ok: true,
                apply_error: None,
                fail_audit_at: None,
                events: Vec::new(),
                confirmations: 0,
                arms: 0,
                applies: 0,
                cancels: 0,
                uncertain: false,
                confirmation_gate: None,
            }
        }
    }
    impl DiagnosticStartPort for FakeStartPort {
        fn authorize(&mut self, attempt_id: &str) -> Result<String, String> {
            self.events.push("authorize");
            if !self.gate {
                return Err("production gate disabled".into());
            }
            if attempt_id.is_empty() || !self.valid_attempt || self.consumed {
                return Err(
                    "missing, invalid, expired, foreign, cancelled, or duplicate attempt".into(),
                );
            }
            self.consumed = true;
            Ok("PreflightAccepted".into())
        }
        fn confirm(
            &mut self,
        ) -> Result<
            (bool, crate::consent_audit::NativeConfirmationTrace),
            (
                String,
                Option<crate::consent_audit::NativeConfirmationTrace>,
            ),
        > {
            self.confirmations += 1;
            self.events.push("confirm");
            if let Some((entered, release)) = self.confirmation_gate.take() {
                let _ = entered.send(());
                let _ = release.recv();
            }
            Ok((
                self.confirmed,
                crate::consent_audit::NativeConfirmationTrace {
                    raw_return_code: Some(if self.confirmed { 6 } else { 7 }),
                    classification: if self.confirmed { "yes" } else { "no" }.into(),
                },
            ))
        }
        fn recheck(&mut self) -> crate::diagnostic::DiagnosticPreflight {
            self.events.push("recheck");
            crate::diagnostic::DiagnosticPreflight::from_matrix(Ok(
                crate::color_engine::matrix_for(&crate::models::Settings::default()),
            ))
        }
        fn audit(
            &mut self,
            outcome: &str,
            _previous: &str,
            _next: &str,
            _reason: &str,
            _native: Option<crate::consent_audit::NativeConfirmationTrace>,
        ) -> Result<(), String> {
            self.events.push(match outcome {
                "confirmationAccepted" => "audit-confirmation",
                "confirmationDeclined" => "audit-declined",
                "authorizationRecorded" => "audit-authorization",
                "armRequested" => "audit-arm-requested",
                "armAccepted" => "audit-arm-accepted",
                "applyRequested" => "audit-apply-requested",
                "applyCommandSent" => "audit-apply-sent",
                _ => "audit-other",
            });
            if self.fail_audit_at == Some(outcome) {
                return Err(format!("fake audit failure at {outcome}"));
            }
            Ok(())
        }
        fn validate(
            &mut self,
            _recheck: crate::diagnostic::DiagnosticPreflight,
        ) -> Result<(), String> {
            self.events.push("validate");
            self.validation_ok
                .then_some(())
                .ok_or("fake validation rejected".into())
        }
        fn confirmation_failed(&mut self, _reason: &str) {
            self.events.push("confirmation-failed");
            self.cancels += 1;
        }
        fn confirmation_declined(&mut self) {
            self.events.push("confirmation-declined");
            self.cancels += 1;
        }
        fn arm(&mut self) -> Result<(), String> {
            self.events.push("arm");
            self.arms += 1;
            self.arm_ok.then_some(()).ok_or("fake arm rejected".into())
        }
        fn request_apply(&mut self) -> Result<(), ApplyRequestError> {
            self.events.push("apply");
            self.applies += 1;
            match self.apply_error.clone() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
        fn cancel_before_apply(&mut self) {
            self.events.push("cancel-or-restore");
            self.cancels += 1;
        }
        fn restore_uncertain(&mut self) {
            self.events.push("restore-uncertain");
            self.cancels += 1;
        }
        fn mark_uncertain(&mut self, _reason: &str) {
            self.events.push("uncertain");
            self.uncertain = true;
        }
    }

    #[test]
    fn command_orchestration_uses_durable_order_before_apply() {
        let mut port = FakeStartPort::default();
        assert!(run_diagnostic_start(&mut port, "fresh-attempt").unwrap());
        assert_eq!(
            port.events,
            [
                "authorize",
                "confirm",
                "audit-confirmation",
                "recheck",
                "validate",
                "audit-authorization",
                "audit-arm-requested",
                "arm",
                "audit-arm-accepted",
                "audit-apply-requested",
                "apply",
                "audit-apply-sent"
            ]
        );
        assert_eq!(port.applies, 1);
    }

    #[test]
    fn disabled_gate_and_invalid_attempts_never_reach_confirmation_or_apply() {
        for (gate, attempt, valid, consumed) in [
            (false, "fresh", true, false),
            (true, "", true, false),
            (true, "expired", false, false),
            (true, "foreign-session", false, false),
            (true, "cancelled", false, true),
            (true, "duplicate", true, true),
        ] {
            let mut port = FakeStartPort {
                gate,
                valid_attempt: valid,
                consumed,
                ..FakeStartPort::default()
            };
            assert!(run_diagnostic_start(&mut port, attempt).is_err());
            assert_eq!(port.confirmations, 0);
            assert_eq!(port.arms, 0);
            assert_eq!(port.applies, 0);
        }
    }

    #[test]
    fn declined_confirmation_and_validation_failure_do_not_apply() {
        let mut declined = FakeStartPort {
            confirmed: false,
            ..FakeStartPort::default()
        };
        assert!(!run_diagnostic_start(&mut declined, "fresh").unwrap());
        assert_eq!(declined.applies, 0);

        let mut invalidated = FakeStartPort {
            validation_ok: false,
            ..FakeStartPort::default()
        };
        assert!(run_diagnostic_start(&mut invalidated, "fresh").is_err());
        assert_eq!(invalidated.arms, 0);
        assert_eq!(invalidated.applies, 0);
    }

    #[test]
    fn audit_failures_before_apply_abort_and_post_send_failure_is_uncertain() {
        for stage in [
            "authorizationRecorded",
            "armRequested",
            "armAccepted",
            "applyRequested",
        ] {
            let mut port = FakeStartPort {
                fail_audit_at: Some(stage),
                ..FakeStartPort::default()
            };
            assert!(run_diagnostic_start(&mut port, "fresh").is_err());
            assert_eq!(port.applies, 0, "stage={stage}");
            assert_eq!(port.cancels, 1, "stage={stage}");
        }

        let mut after_send = FakeStartPort {
            fail_audit_at: Some("applyCommandSent"),
            ..FakeStartPort::default()
        };
        let error = run_diagnostic_start(&mut after_send, "fresh").unwrap_err();
        assert!(error.contains("incierto"));
        assert_eq!(after_send.applies, 1);
        assert!(after_send.uncertain);
        assert_eq!(after_send.cancels, 1);
        assert!(after_send.events.contains(&"restore-uncertain"));
    }

    #[test]
    fn possibly_sent_ipc_failure_is_uncertain_but_pre_send_failure_is_not() {
        let mut maybe_sent = FakeStartPort {
            apply_error: Some(ApplyRequestError::MayHaveBeenSent(
                "pipe interrupted".into(),
            )),
            ..FakeStartPort::default()
        };
        assert!(
            run_diagnostic_start(&mut maybe_sent, "fresh")
                .unwrap_err()
                .contains("incierto")
        );
        assert!(maybe_sent.uncertain);

        let mut rejected = FakeStartPort {
            apply_error: Some(ApplyRequestError::RejectedBeforeSend("not armed".into())),
            ..FakeStartPort::default()
        };
        assert_eq!(
            run_diagnostic_start(&mut rejected, "fresh").unwrap_err(),
            "not armed"
        );
        assert!(!rejected.uncertain);
        assert_eq!(rejected.applies, 1);
    }

    #[test]
    fn duplicate_direct_start_is_one_shot_and_never_sends_twice() {
        let mut port = FakeStartPort::default();
        assert!(run_diagnostic_start(&mut port, "fresh").unwrap());
        assert!(run_diagnostic_start(&mut port, "fresh").is_err());
        assert_eq!(port.confirmations, 1);
        assert_eq!(port.applies, 1);
    }

    #[test]
    fn cancel_racing_start_is_serialized_by_the_same_service_mutex() {
        use std::sync::{Arc, Mutex, mpsc};
        use std::thread;

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut port = FakeStartPort::default();
        port.confirmation_gate = Some((entered_tx, release_rx));
        let shared = Arc::new(Mutex::new(port));
        let start_state = Arc::clone(&shared);
        let start = thread::spawn(move || {
            let mut port = start_state.lock().unwrap();
            run_diagnostic_start(&mut *port, "fresh")
        });
        entered_rx.recv().unwrap();

        let (cancel_started_tx, cancel_started_rx) = mpsc::channel();
        let cancel_state = Arc::clone(&shared);
        let cancel = thread::spawn(move || {
            cancel_started_tx.send(()).unwrap();
            cancel_state.lock().unwrap().cancel_before_apply();
        });
        cancel_started_rx.recv().unwrap();
        assert!(
            shared.try_lock().is_err(),
            "start holds the same state lock as cancel"
        );
        release_tx.send(()).unwrap();
        assert!(start.join().unwrap().unwrap());
        cancel.join().unwrap();
        let port = shared.lock().unwrap();
        assert_eq!(port.applies, 1);
        let apply = port
            .events
            .iter()
            .position(|event| *event == "apply")
            .unwrap();
        let cancel = port
            .events
            .iter()
            .rposition(|event| *event == "cancel-or-restore")
            .unwrap();
        assert!(apply < cancel);
    }

    #[test]
    fn stale_start_attempt_is_rejected_before_confirmation_or_watchdog_apply() {
        let root = std::env::temp_dir().join(format!(
            "vyra-start-gate-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut diagnostic = DiagnosticService::new(root.join("diagnostic-events.jsonl"));
        let error = authorize_diagnostic_start(&mut diagnostic, "unexpected-or-stale").unwrap_err();
        assert!(error.contains("bloqueado por incidente"));
        let report = diagnostic.report();
        assert_eq!(
            report.phase,
            crate::diagnostic::DiagnosticPhase::ConsentIncidentLockdown
        );
        assert!(!report.confirmation_accepted);
        assert!(!report.can_request_confirmation);
        assert_eq!(report.application_accepted, None);
        assert_eq!(report.helper_pid, None);
        assert!(!root.join(crate::watchdog::session::JOURNAL).exists());
        let audit = std::fs::read_to_string(root.join("consent-audit.jsonl")).unwrap();
        assert!(audit.contains("start_diagnostic"));
        assert!(audit.contains("rejected"));
        for name in ["diagnostic-events.jsonl", "consent-audit.jsonl"] {
            let path = root.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn query_cancel_and_handoff_service_transitions_never_report_application() {
        let root = std::env::temp_dir().join(format!(
            "vyra-read-cancel-handoff-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut diagnostic = DiagnosticService::new(root.join("diagnostic-events.jsonl"));
        let read = diagnostic.report();
        assert_eq!(read.application_accepted, None);
        assert_eq!(read.helper_pid, None);
        let cancelled = diagnostic.cancel().unwrap();
        assert_eq!(cancelled.stage, "cancelled");
        assert_eq!(cancelled.application_accepted, None);
        assert!(!diagnostic.active());
        for name in ["diagnostic-events.jsonl", "consent-audit.jsonl"] {
            let path = root.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(root).unwrap();
    }
}
