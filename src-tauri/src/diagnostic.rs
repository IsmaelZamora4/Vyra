//! Read-only preparation. No production color setter is reachable from this module.
use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_DURATION_SECONDS: u8 = 10;
pub const MAX_PREFLIGHT_WAIT_SECONDS: u8 = 60;
pub const SATURATION: i16 = 5;
pub const RECOVERY_BLOCKER: &str = crate::watchdog::RECOVERY_BLOCKER;
pub const CONFIRMATION_REQUIRED: &str = "El watchdog está armado. La recuperación después de una escritura real todavía no está validada; confirma el ensayo en Windows para continuar.";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticPhase {
    NotStarted,
    PreflightPending,
    PreflightAccepted,
    ConfirmationAvailable,
    Confirming,
    ConsentIncidentLockdown,
    SessionExpired,
    RecoveryPending,
    ExternalConflict,
    WatchdogError,
    Applying,
    Applied,
    OutcomeUncertain,
    AuditPersistenceFailed,
    Cancelled,
    Restored,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticBlocker {
    BackendMismatch,
    FiltersActive,
    RecoveryPending,
    WindowsReadFailed,
    HelperUnavailable,
    PreflightExpired,
    ExternalConflict,
    WatchdogError,
    ConfirmationDeclined,
    NativeConfirmationFailed,
    SessionExpired,
    ConsentIncidentLockdown,
    ApplyOutcomeUncertain,
    AuditPersistenceFailed,
}

pub fn require_independent_recovery() -> Result<(), String> {
    // No environment variable, frontend argument or persisted preference can bypass this gate.
    Err(RECOVERY_BLOCKER.into())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub attempt_id: Option<String>,
    pub stage: String,
    pub phase: DiagnosticPhase,
    pub blocker_code: Option<DiagnosticBlocker>,
    pub max_duration_seconds: u8,
    pub max_preflight_wait_seconds: u8,
    pub saturation: i16,
    pub independent_recovery_ready: bool,
    pub current_state_read_accepted: Option<bool>,
    pub confirmation_accepted: bool,
    pub application_accepted: Option<bool>,
    pub application_verified: Option<bool>,
    pub restoration_accepted: Option<bool>,
    pub can_request_confirmation: bool,
    pub reason: String,
    pub timestamp_ms: u128,
    pub watchdog_state: String,
    pub helper_verified: bool,
    pub helper_pid: Option<u32>,
    pub helper_initialization_accepted: Option<bool>,
    pub helper_state_read_accepted: Option<bool>,
    pub helper_journal_durable: Option<bool>,
    pub helper_same_windows_session: Option<bool>,
    pub restoration_verified: Option<bool>,
}

impl Default for DiagnosticReport {
    fn default() -> Self {
        Self {
            attempt_id: None,
            stage: "notStarted".into(),
            phase: DiagnosticPhase::NotStarted,
            blocker_code: None,
            max_duration_seconds: MAX_DURATION_SECONDS,
            max_preflight_wait_seconds: MAX_PREFLIGHT_WAIT_SECONDS,
            saturation: SATURATION,
            independent_recovery_ready: false,
            current_state_read_accepted: None,
            confirmation_accepted: false,
            application_accepted: None,
            application_verified: None,
            restoration_accepted: None,
            can_request_confirmation: false,
            reason: RECOVERY_BLOCKER.into(),
            timestamp_ms: 0,
            watchdog_state: "notStarted".into(),
            helper_verified: false,
            helper_pid: None,
            helper_initialization_accepted: None,
            helper_state_read_accepted: None,
            helper_journal_durable: None,
            helper_same_windows_session: None,
            restoration_verified: None,
        }
    }
}

pub struct DiagnosticService {
    path: PathBuf,
    audit: crate::consent_audit::ConsentAudit,
    attempt: Option<crate::consent_audit::ConsentAttempt>,
    report: DiagnosticReport,
    client: Option<crate::watchdog::client::Client>,
    preflight_matrix: Option<crate::color_engine::magnification::Matrix>,
    preflight_expires_at: Option<Instant>,
    confirming: bool,
    audit_failure: Option<String>,
}
pub struct DiagnosticPreflight {
    read_accepted: Option<bool>,
    matrix: Option<crate::color_engine::magnification::Matrix>,
    blocker: Option<DiagnosticBlocker>,
    reason: String,
}
impl DiagnosticPreflight {
    pub fn blocked(blocker: DiagnosticBlocker, reason: &str) -> Self {
        Self {
            read_accepted: None,
            matrix: None,
            blocker: Some(blocker),
            reason: reason.into(),
        }
    }
    pub fn from_matrix(result: Result<crate::color_engine::magnification::Matrix, String>) -> Self {
        match result {
            Ok(matrix) => Self {
                read_accepted: Some(true),
                matrix: Some(matrix),
                blocker: None,
                reason: CONFIRMATION_REQUIRED.into(),
            },
            Err(reason) => Self {
                read_accepted: Some(false),
                matrix: None,
                blocker: Some(DiagnosticBlocker::WindowsReadFailed),
                reason,
            },
        }
    }
}
impl DiagnosticService {
    pub fn new(path: PathBuf) -> Self {
        let audit_path = path.with_file_name("consent-audit.jsonl");
        Self {
            path,
            audit: crate::consent_audit::ConsentAudit::new(audit_path),
            attempt: None,
            report: DiagnosticReport::default(),
            client: None,
            preflight_matrix: None,
            preflight_expires_at: None,
            confirming: false,
            audit_failure: None,
        }
    }
    pub fn report(&mut self) -> DiagnosticReport {
        self.refresh();
        self.report.clone()
    }

    fn root(&self) -> &std::path::Path {
        self.path.parent().expect("Directorio de datos")
    }

    pub fn audit_command(
        &self,
        command: &str,
        outcome: &str,
        previous: &str,
        next: &str,
        reason: &str,
        native: Option<crate::consent_audit::NativeConfirmationTrace>,
    ) -> Result<(), String> {
        self.audit.record(
            self.report.attempt_id.as_deref(),
            self.attempt
                .as_ref()
                .and_then(crate::consent_audit::ConsentAttempt::session_fingerprint),
            command,
            outcome,
            std::process::id(),
            self.client.as_ref().map(|client| client.child.id()),
            previous,
            next,
            reason,
            native,
        )
    }

    fn mark_audit_persistence_failed(&mut self, error: &str, context: &str) {
        self.audit_failure = Some(format!(
            "{context}; no se pudo persistir la auditoría: {error}. Revisa también el resultado durable del watchdog."
        ));
        self.report.phase = DiagnosticPhase::AuditPersistenceFailed;
        self.report.blocker_code = Some(DiagnosticBlocker::AuditPersistenceFailed);
        self.report.stage = "auditIncomplete".into();
        self.report.reason = self.audit_failure.clone().unwrap();
        let _ = self.persist_current_report();
    }

    pub fn claim_attempt(&mut self, requested_id: &str) -> Result<(), String> {
        self.refresh();
        let session = self.client.as_ref().map(|client| client.session.as_str());
        let fingerprint = session.map(crate::consent_audit::fingerprint_session);
        let attempt = self
            .attempt
            .as_mut()
            .ok_or("No hay intento de autorización vigente.")?;
        attempt
            .claim(requested_id, fingerprint.as_deref(), Instant::now())
            .map_err(|error| format!("Intento de autorización rechazado: {error:?}."))
    }

    fn owns_active_journal(&self) -> bool {
        let Some(client) = self.client.as_ref().filter(|client| {
            !client.finished && client.state != "exited" && client.state != "failed"
        }) else {
            return false;
        };
        crate::watchdog::session::load(self.root())
            .is_ok_and(|journal| journal.session == client.session)
    }

    fn owns_preflight_journal(&self) -> bool {
        self.client
            .as_ref()
            .is_some_and(|client| client.state == "preflight")
            && self.owns_active_journal()
            && crate::watchdog::session::load(self.root()).is_ok_and(|journal| {
                journal.state == crate::watchdog::session::State::Prepared
                    && !journal.may_have_written
            })
    }

    fn pending(&self) -> bool {
        match self
            .root()
            .join(crate::watchdog::session::JOURNAL)
            .try_exists()
        {
            Ok(false) => false,
            Ok(true) => !self.owns_active_journal(),
            Err(_) => true,
        }
    }

    fn refresh(&mut self) {
        let mut audit_failure: Option<(String, &'static str)> = None;
        if let Some(client) = &mut self.client {
            let watchdog_pid = Some(client.child.id());
            for event in client.poll() {
                match event {
                    crate::watchdog::protocol::Event::Applied {
                        accepted, verified, ..
                    } => {
                        self.report.application_accepted = Some(accepted);
                        self.report.application_verified = Some(verified);
                        self.report.stage = if accepted && verified {
                            "applied".into()
                        } else {
                            "failed".into()
                        };
                        self.report.phase = if accepted && verified {
                            DiagnosticPhase::Applied
                        } else {
                            DiagnosticPhase::OutcomeUncertain
                        };
                        let acceptance_outcome = if accepted {
                            "applyAccepted"
                        } else {
                            "applyRejected"
                        };
                        let mut audit_error = self
                            .audit
                            .record(
                                self.report.attempt_id.as_deref(),
                                self.attempt.as_ref().and_then(
                                    crate::consent_audit::ConsentAttempt::session_fingerprint,
                                ),
                                "watchdog",
                                acceptance_outcome,
                                std::process::id(),
                                watchdog_pid,
                                "Applying",
                                if accepted {
                                    "ApplyAccepted"
                                } else {
                                    "ApplyRejected"
                                },
                                if accepted {
                                    "Watchdog aceptó Apply"
                                } else {
                                    "Watchdog rechazó Apply"
                                },
                                None,
                            )
                            .err();
                        if audit_error.is_none() {
                            audit_error = self
                                .audit
                                .record(
                                    self.report.attempt_id.as_deref(),
                                    self.attempt.as_ref().and_then(
                                        crate::consent_audit::ConsentAttempt::session_fingerprint,
                                    ),
                                    "watchdog",
                                    if verified {
                                        "targetReadVerified"
                                    } else {
                                        "targetReadMismatch"
                                    },
                                    std::process::id(),
                                    watchdog_pid,
                                    "ApplyAccepted",
                                    if verified {
                                        "Applied"
                                    } else {
                                        "OutcomeUncertain"
                                    },
                                    if verified {
                                        "Lectura posterior coincide con la matriz objetivo"
                                    } else {
                                        "Lectura posterior no coincide con la matriz objetivo"
                                    },
                                    None,
                                )
                                .err();
                        }
                        if let Some(error) = audit_error {
                            audit_failure = Some((error, "Watchdog informó el resultado de Apply"));
                        }
                    }
                    crate::watchdog::protocol::Event::Finished {
                        accepted,
                        verified,
                        reason,
                        ..
                    } => {
                        self.report.restoration_accepted = accepted;
                        self.report.restoration_verified = Some(verified);
                        let mut audit_error = self
                            .audit
                            .record(
                                self.report.attempt_id.as_deref(),
                                self.attempt.as_ref().and_then(
                                    crate::consent_audit::ConsentAttempt::session_fingerprint,
                                ),
                                "watchdog",
                                match accepted {
                                    Some(true) => "restorationAccepted",
                                    Some(false) => "restorationNotAccepted",
                                    None => "restorationNotRequired",
                                },
                                std::process::id(),
                                watchdog_pid,
                                "Restoring",
                                match accepted {
                                    Some(true) => "RestorationAccepted",
                                    Some(false) => "RestorationNotAccepted",
                                    None => "RestorationNotRequired",
                                },
                                &reason,
                                None,
                            )
                            .err();
                        if audit_error.is_none() {
                            audit_error = self
                                .audit
                                .record(
                                    self.report.attempt_id.as_deref(),
                                    self.attempt.as_ref().and_then(
                                        crate::consent_audit::ConsentAttempt::session_fingerprint,
                                    ),
                                    "watchdog",
                                    if verified {
                                        "originalReadVerified"
                                    } else {
                                        "originalReadUnverified"
                                    },
                                    std::process::id(),
                                    watchdog_pid,
                                    "Restoring",
                                    if verified {
                                        "Restored"
                                    } else {
                                        "OutcomeUncertain"
                                    },
                                    if verified {
                                        "Nueva lectura coincide con la matriz original"
                                    } else {
                                        "Nueva lectura no coincide con la matriz original"
                                    },
                                    None,
                                )
                                .err();
                        }
                        if self.report.application_accepted.is_some() {
                            self.report.stage = if verified { "restored" } else { "failed" }.into();
                            self.report.phase = if verified {
                                DiagnosticPhase::Restored
                            } else {
                                DiagnosticPhase::Failed
                            };
                        }
                        if !verified {
                            self.report.reason = reason;
                        }
                        if let Some(error) = audit_error {
                            audit_failure = Some((
                                error,
                                "Watchdog terminó, pero no se pudo persistir el resultado de restauración",
                            ));
                        }
                    }
                    crate::watchdog::protocol::Event::Ready {
                        helper_initialized,
                        helper_read_accepted,
                        journal_durable,
                        same_windows_session,
                        ..
                    } => {
                        self.report.helper_initialization_accepted = Some(helper_initialized);
                        self.report.helper_state_read_accepted = Some(helper_read_accepted);
                        self.report.helper_journal_durable = Some(journal_durable);
                        self.report.helper_same_windows_session = Some(same_windows_session);
                    }
                    crate::watchdog::protocol::Event::Armed { .. } => {
                        self.report.phase = DiagnosticPhase::Applying;
                    }
                }
            }
            self.report.watchdog_state = client.state.clone();
            let result_path = self
                .path
                .parent()
                .unwrap()
                .join(crate::watchdog::session::RESULT);
            if let Ok(bytes) = std::fs::read(result_path) {
                if let Ok(outcome) =
                    serde_json::from_slice::<crate::watchdog::session::Outcome>(&bytes)
                {
                    if outcome.session == client.session {
                        self.report.application_accepted = outcome.application_accepted;
                        self.report.application_verified = outcome.application_verified;
                        self.report.restoration_accepted = outcome.restoration_accepted;
                        self.report.restoration_verified = Some(outcome.restoration_verified);
                        if outcome.application_accepted.is_some() {
                            self.report.stage = if outcome.restoration_verified {
                                "restored"
                            } else {
                                "failed"
                            }
                            .into();
                            self.report.phase = if outcome.restoration_verified {
                                DiagnosticPhase::Restored
                            } else {
                                DiagnosticPhase::Failed
                            };
                        } else if outcome.cause == "preflightTimeout" {
                            self.report.stage = "expired".into();
                            self.report.phase = DiagnosticPhase::SessionExpired;
                            self.report.blocker_code = Some(DiagnosticBlocker::PreflightExpired);
                            self.report.reason =
                                "El preflight expiró antes de la confirmación; no hubo Apply."
                                    .into();
                        }
                        self.report.watchdog_state = if outcome.journal_retained {
                            "failed"
                        } else {
                            "restored"
                        }
                        .into();
                        client.finished = true;
                        if let Some(error) = outcome.error {
                            self.report.reason = error;
                            if self.report.reason.to_lowercase().contains("conflicto") {
                                self.report.phase = DiagnosticPhase::ExternalConflict;
                                self.report.blocker_code =
                                    Some(DiagnosticBlocker::ExternalConflict);
                            } else {
                                self.report.phase = DiagnosticPhase::WatchdogError;
                                self.report.blocker_code = Some(DiagnosticBlocker::WatchdogError);
                            }
                        }
                    }
                }
            }
            if let Some(error) = &client.last_error {
                if self.report.watchdog_state != "restored" {
                    self.report.reason = error.clone();
                    self.report.phase = DiagnosticPhase::WatchdogError;
                    self.report.blocker_code = Some(DiagnosticBlocker::WatchdogError);
                }
            }
        } else if self.pending() {
            self.report.watchdog_state = "pendingRecovery".into();
            self.report.reason = "Existe un diario watchdog pendiente; usa Restaurar. No se eliminará para desbloquear la prueba.".into();
            self.report.phase = DiagnosticPhase::RecoveryPending;
            self.report.blocker_code = Some(DiagnosticBlocker::RecoveryPending);
        }
        if let Some((error, context)) = audit_failure {
            self.mark_audit_persistence_failed(&error, context);
        }
        if let Some(failure) = &self.audit_failure {
            self.report.phase = DiagnosticPhase::AuditPersistenceFailed;
            self.report.blocker_code = Some(DiagnosticBlocker::AuditPersistenceFailed);
            self.report.stage = "auditIncomplete".into();
            self.report.reason = failure.clone();
        }
        let expired = self
            .preflight_expires_at
            .is_some_and(|deadline| Instant::now() >= deadline);
        let client_state = self
            .client
            .as_ref()
            .filter(|client| client.read_only_preflight_passed && !client.finished)
            .map(|client| client.state.as_str());
        let availability = if self.report.stage == "awaitingConfirmation"
            && !crate::watchdog::REAL_DIAGNOSTIC_ENABLED
        {
            Err(DiagnosticBlocker::ConsentIncidentLockdown)
        } else if self.report.stage == "awaitingConfirmation" {
            confirmation_gate(
                &self.report,
                client_state,
                self.owns_preflight_journal(),
                self.pending(),
                expired,
            )
        } else {
            Err(DiagnosticBlocker::WatchdogError)
        };
        self.report.can_request_confirmation = availability.is_ok();
        if let Err(blocker) = availability
            && self.report.stage == "awaitingConfirmation"
        {
            if blocker == DiagnosticBlocker::PreflightExpired {
                self.report.phase = DiagnosticPhase::SessionExpired;
                self.report.reason = "El preflight caducó; prepara una sesión nueva.".into();
            } else if blocker == DiagnosticBlocker::RecoveryPending {
                self.report.phase = DiagnosticPhase::RecoveryPending;
                self.report.reason =
                    "Existe una recuperación pendiente; se conserva su diario.".into();
            } else if blocker == DiagnosticBlocker::ConsentIncidentLockdown {
                self.report.phase = DiagnosticPhase::ConsentIncidentLockdown;
                self.report.reason = "Bloqueo por incidente: el ensayo visual está deshabilitado; solo se permite preflight de lectura y recuperación manual.".into();
            } else if self.report.phase != DiagnosticPhase::ExternalConflict {
                self.report.phase = DiagnosticPhase::WatchdogError;
                if self.report.reason.is_empty() {
                    self.report.reason =
                        "El watchdog no confirmó todas las condiciones requeridas.".into();
                }
            }
            self.report.blocker_code = Some(blocker);
        }
        if self.report.can_request_confirmation {
            self.report.phase = if self.confirming {
                DiagnosticPhase::Confirming
            } else {
                DiagnosticPhase::ConfirmationAvailable
            };
            self.report.blocker_code = None;
        }
    }

    pub fn ensure_idle(&mut self) -> Result<(), String> {
        self.refresh();
        if self.active() || self.pending() || self.confirming {
            return Err(
                "Sesión watchdog activa o recuperación pendiente. Restaura primero.".into(),
            );
        }
        self.client.take();
        Ok(())
    }
    pub fn active(&self) -> bool {
        self.client
            .as_ref()
            .is_some_and(|client| !client.finished && client.state != "exited")
    }
    pub fn needs_restoration(&mut self) -> bool {
        self.refresh();
        self.active() || self.pending()
    }
    pub fn status_override(&mut self) -> Option<crate::color_engine::EngineStatus> {
        self.refresh();
        if self.pending() && !self.active() {
            Some(crate::color_engine::EngineStatus {
                enabled: false,
                recovery_required: true,
                description: format!("Recuperación watchdog pendiente: {}", self.report.reason),
            })
        } else {
            None
        }
    }

    pub fn prepare_watchdog(&mut self) -> Result<DiagnosticReport, String> {
        self.ensure_idle()?;
        self.report.stage = "preflightPending".into();
        self.report.phase = DiagnosticPhase::PreflightPending;
        self.report.blocker_code = None;
        self.report.reason = "Comprobando watchdog, sesión y lectura de Magnification.".into();
        self.record(self.report.clone())?;
        let result = crate::watchdog::client::self_check()
            .and_then(|_| crate::watchdog::client::Client::prepare_native(self.root()));
        match result {
            Ok(client) => {
                if !client.read_only_preflight_passed {
                    return Err(
                        "Watchdog no confirmó un preflight completo de solo lectura.".into(),
                    );
                }
                self.report.helper_verified = true;
                self.report.helper_pid = Some(client.child.id());
                self.report.watchdog_state = client.state.clone();
                if client.read_only_preflight_passed {
                    self.report.helper_initialization_accepted = Some(true);
                    self.report.helper_state_read_accepted = Some(true);
                    self.report.helper_journal_durable = Some(true);
                    self.report.helper_same_windows_session = Some(true);
                }
                self.report.stage = "awaitingConfirmation".into();
                self.report.reason = CONFIRMATION_REQUIRED.into();
                self.report.phase = DiagnosticPhase::PreflightAccepted;
                self.report.blocker_code = None;
                self.preflight_expires_at =
                    Some(Instant::now() + Duration::from_secs(MAX_PREFLIGHT_WAIT_SECONDS.into()));
                let mut attempt = crate::consent_audit::ConsentAttempt::new()?;
                attempt.bind_session(
                    &client.session,
                    self.preflight_expires_at.expect("deadline recién asignado"),
                );
                self.report.attempt_id = Some(attempt.id().to_owned());
                self.attempt = Some(attempt);
                self.client = Some(client);
            }
            Err(error) => {
                self.report.watchdog_state = "failed".into();
                self.report.reason = error;
                self.report.phase = DiagnosticPhase::WatchdogError;
                self.report.blocker_code = Some(DiagnosticBlocker::WatchdogError);
            }
        }
        self.record(self.report.clone())
    }

    pub fn restore_watchdog(&mut self) -> Result<(), String> {
        self.finish_watchdog(false)
    }

    pub fn begin_confirmation(&mut self) -> Result<(), String> {
        self.refresh();
        if !crate::watchdog::REAL_DIAGNOSTIC_ENABLED {
            return Err(self.report.reason.clone());
        }
        if !self.report.can_request_confirmation {
            return Err(self.report.reason.clone());
        }
        self.confirming = true;
        self.report.phase = DiagnosticPhase::Confirming;
        self.report.reason = "Esperando confirmación nativa de Windows.".into();
        self.record(self.report.clone()).map(|_| ())
    }

    pub fn reject_confirmation(&mut self) -> Result<DiagnosticReport, String> {
        self.confirming = false;
        let mut report = self.cancel()?;
        report.blocker_code = Some(DiagnosticBlocker::ConfirmationDeclined);
        report.reason = "Ensayo cancelado en la confirmación nativa; no se aplicó color.".into();
        self.record(report)
    }

    pub fn fail_confirmation(&mut self, reason: &str) -> Result<DiagnosticReport, String> {
        self.confirming = false;
        let cleanup_error = self.cancel().err();
        let mut report = self.report.clone();
        report.stage = "failed".into();
        report.phase = DiagnosticPhase::WatchdogError;
        report.blocker_code = Some(DiagnosticBlocker::NativeConfirmationFailed);
        report.can_request_confirmation = false;
        report.reason = match cleanup_error {
            Some(error) => format!("{reason} Limpieza watchdog incompleta: {error}"),
            None => reason.to_owned(),
        };
        self.record(report)
    }

    pub fn record_incident_lockdown(&mut self) -> Result<DiagnosticReport, String> {
        self.confirming = false;
        self.report.can_request_confirmation = false;
        self.report.phase = DiagnosticPhase::ConsentIncidentLockdown;
        self.report.blocker_code = Some(DiagnosticBlocker::ConsentIncidentLockdown);
        self.report.reason = "Bloqueo por incidente: el ensayo visual está deshabilitado; solo se permite preflight de lectura y recuperación manual.".into();
        self.record(self.report.clone())
    }

    fn finish_watchdog(&mut self, cancel_before_apply: bool) -> Result<(), String> {
        self.refresh();
        let audit_error;
        if self.active() {
            let cancel = cancel_before_apply
                && self
                    .client
                    .as_ref()
                    .is_some_and(|client| matches!(client.state.as_str(), "preflight" | "armed"));
            if cancel {
                audit_error = self
                    .audit_command(
                        "cancel_diagnostic",
                        "cancelRequested",
                        &format!("{:?}", self.report.phase),
                        "Cancelling",
                        "Persistir antes de solicitar Cancel",
                        None,
                    )
                    .err();
                self.client.as_mut().unwrap().request_cancel()?;
            } else {
                audit_error = self
                    .audit_command(
                        "restore_colors",
                        "restorationRequested",
                        &format!("{:?}", self.report.phase),
                        "Restoring",
                        "Persistir antes de solicitar Restore; recuperación tiene prioridad",
                        None,
                    )
                    .err();
                self.client.as_mut().unwrap().request_restore()?;
            }
        } else if self.pending() {
            audit_error = self
                .audit_command(
                    "restore_colors",
                    "restorationRequested",
                    &format!("{:?}", self.report.phase),
                    "Restoring",
                    "Persistir antes de iniciar recuperación; recuperación tiene prioridad",
                    None,
                )
                .err();
            self.client = Some(crate::watchdog::client::Client::recover_native(
                self.root(),
            )?);
        } else {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            self.refresh();
            if self.client.as_ref().is_some_and(|client| client.finished) {
                if !self.pending() && self.report.restoration_verified == Some(true) {
                    if let Some(error) = audit_error {
                        self.report.phase = DiagnosticPhase::AuditPersistenceFailed;
                        self.report.blocker_code = Some(DiagnosticBlocker::AuditPersistenceFailed);
                        self.report.stage = "auditIncomplete".into();
                        self.report.reason = format!(
                            "Restauración verificada por watchdog, pero no se pudo registrar la solicitud: {error}"
                        );
                        let _ = self.persist_current_report();
                        return Err(self.report.reason.clone());
                    }
                    return Ok(());
                } else {
                    return Err(self.report.reason.clone());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(
            "Watchdog no confirmó restauración dentro del plazo de espera; diario conservado."
                .into(),
        )
    }

    fn record(&mut self, report: DiagnosticReport) -> Result<DiagnosticReport, String> {
        self.report = report;
        self.refresh();
        self.persist_current_report()
    }

    fn persist_current_report(&mut self) -> Result<DiagnosticReport, String> {
        self.report.timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut bytes = serde_json::to_vec(&self.report).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("No se pudo registrar el diagnóstico: {e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("No se pudo registrar el diagnóstico: {e}"))?;
        Ok(self.report.clone())
    }

    pub fn prepare(&mut self, preflight: DiagnosticPreflight) -> Result<DiagnosticReport, String> {
        self.audit_failure = None;
        self.attempt = None;
        self.preflight_matrix = preflight.matrix;
        self.preflight_expires_at = None;
        let phase = match (preflight.read_accepted, preflight.blocker) {
            (_, Some(DiagnosticBlocker::RecoveryPending)) => DiagnosticPhase::RecoveryPending,
            (_, Some(DiagnosticBlocker::ExternalConflict)) => DiagnosticPhase::ExternalConflict,
            (_, Some(DiagnosticBlocker::WatchdogError)) => DiagnosticPhase::WatchdogError,
            (Some(true), None) => DiagnosticPhase::PreflightAccepted,
            _ => DiagnosticPhase::Failed,
        };
        let report = DiagnosticReport {
            stage: "blocked".into(),
            phase,
            blocker_code: preflight.blocker,
            current_state_read_accepted: preflight.read_accepted,
            reason: preflight.reason,
            ..DiagnosticReport::default()
        };
        self.record(report)
    }
    pub fn cancel(&mut self) -> Result<DiagnosticReport, String> {
        self.confirming = false;
        if let Some(attempt) = &mut self.attempt {
            attempt.revoke();
        }
        self.refresh();
        let application_attempted = self.report.stage == "applying"
            || self.report.application_accepted.is_some()
            || self
                .client
                .as_ref()
                .is_some_and(|client| matches!(client.state.as_str(), "applying" | "applied"));
        if self.needs_restoration() {
            self.finish_watchdog(!application_attempted)?;
        }
        let mut report = self.report.clone();
        if application_attempted {
            report.stage = if report.restoration_verified == Some(true) {
                "restored"
            } else {
                "failed"
            }
            .into();
            if report.restoration_verified == Some(true) {
                report.reason =
                    "Ensayo terminado; lectura de retorno de la matriz original aceptada.".into();
            }
        } else {
            report.stage = "cancelled".into();
            report.phase = DiagnosticPhase::Cancelled;
            report.blocker_code = None;
            report.reason = "Cancelado sin iniciar la aplicación de color.".into();
        }
        self.record(report)
    }
    pub fn validate_after_native_confirmation(
        &mut self,
        recheck: DiagnosticPreflight,
    ) -> Result<(), String> {
        if !crate::watchdog::REAL_DIAGNOSTIC_ENABLED {
            let _ = self.record_incident_lockdown();
            return Err("Apply bloqueado por incidente de consentimiento no acreditado.".into());
        }
        self.refresh();
        self.confirming = false;
        if !self.report.can_request_confirmation {
            self.report.phase = if self.report.phase == DiagnosticPhase::SessionExpired {
                DiagnosticPhase::SessionExpired
            } else if self.pending() {
                DiagnosticPhase::RecoveryPending
            } else {
                DiagnosticPhase::WatchdogError
            };
            self.report.blocker_code = Some(match self.report.phase {
                DiagnosticPhase::SessionExpired => DiagnosticBlocker::PreflightExpired,
                DiagnosticPhase::RecoveryPending => DiagnosticBlocker::RecoveryPending,
                _ => DiagnosticBlocker::WatchdogError,
            });
            self.report.reason =
                "La sesión de preflight ya no está disponible; no se enviará Apply.".into();
            let _ = self.record(self.report.clone());
            return Err(self.report.reason.clone());
        }
        if let Some(blocker) = recheck.blocker {
            let reason = recheck.reason;
            let _ = self.finish_watchdog(true);
            self.report.stage = "failed".into();
            self.report.blocker_code = Some(blocker);
            self.report.phase = phase_for_blocker(blocker);
            self.report.reason = reason.clone();
            let _ = self.record(self.report.clone());
            return Err(reason);
        }
        let (Some(original), Some(current)) = (self.preflight_matrix, recheck.matrix) else {
            let reason = "La segunda lectura de Windows no produjo una matriz.".to_string();
            let _ = self.finish_watchdog(true);
            self.report.stage = "failed".into();
            self.report.blocker_code = Some(DiagnosticBlocker::WindowsReadFailed);
            self.report.phase = DiagnosticPhase::Failed;
            self.report.reason = reason.clone();
            let _ = self.record(self.report.clone());
            return Err(reason);
        };
        if !crate::color_engine::magnification::matrices_match(original, current) {
            let reason =
                "Conflicto externo: la matriz cambió desde el preflight; no se aplicará color."
                    .to_string();
            let _ = self.finish_watchdog(true);
            self.report.stage = "failed".into();
            self.report.blocker_code = Some(DiagnosticBlocker::ExternalConflict);
            self.report.phase = DiagnosticPhase::ExternalConflict;
            self.report.reason = reason.clone();
            let _ = self.record(self.report.clone());
            return Err(reason);
        }
        Ok(())
    }

    pub fn arm_after_native_confirmation(&mut self) -> Result<(), String> {
        self.report.confirmation_accepted = true;
        let arm_result = self
            .client
            .as_mut()
            .ok_or("No hay una sesión watchdog para el ensayo.")?
            .arm_after_confirmation();
        if let Err(error) = arm_result {
            self.refresh();
            self.report.blocker_code = Some(if error.to_lowercase().contains("conflicto") {
                DiagnosticBlocker::ExternalConflict
            } else if error.to_lowercase().contains("expired")
                || error.to_lowercase().contains("venc")
            {
                DiagnosticBlocker::PreflightExpired
            } else {
                DiagnosticBlocker::WatchdogError
            });
            self.report.phase = phase_for_blocker(self.report.blocker_code.unwrap());
            self.report.stage = "failed".into();
            self.report.reason = error.clone();
            let _ = self.record(self.report.clone());
            return Err(error);
        }
        self.report.stage = "armed".into();
        self.report.watchdog_state = "armed".into();
        self.report.blocker_code = None;
        self.report.reason =
            "Watchdog confirmó un armado fresco; Apply aún no se ha solicitado.".into();
        self.record(self.report.clone())?;
        Ok(())
    }

    pub fn request_diagnostic_apply(
        &mut self,
    ) -> Result<(), crate::watchdog::client::ApplyRequestError> {
        let apply_result = self
            .client
            .as_mut()
            .ok_or_else(|| {
                crate::watchdog::client::ApplyRequestError::RejectedBeforeSend(
                    "No hay una sesión watchdog después del armado.".into(),
                )
            })?
            .request_apply();
        if let Err(error) = apply_result {
            if error.may_have_been_sent() {
                self.mark_apply_outcome_uncertain(error.reason());
            }
            return Err(error);
        }
        self.report.stage = "applying".into();
        self.report.phase = DiagnosticPhase::Applying;
        self.report.blocker_code = None;
        self.report.watchdog_state = "applying".into();
        self.report.reason = "Ensayo iniciado. Restauración independiente armada; observa el estado y restaura ahora si lo necesitas.".into();
        // Do not poll the watchdog here: the command boundary must persist
        // applyCommandSent before any queued Applied acknowledgment is audited.
        if let Err(error) = self.persist_current_report() {
            self.mark_apply_outcome_uncertain(&error);
            return Err(crate::watchdog::client::ApplyRequestError::MayHaveBeenSent(
                format!("Apply pudo enviarse; falló la persistencia local: {error}"),
            ));
        }
        Ok(())
    }

    pub fn mark_apply_outcome_uncertain(&mut self, reason: &str) {
        self.confirming = false;
        self.report.stage = "outcomeUncertain".into();
        self.report.phase = DiagnosticPhase::OutcomeUncertain;
        self.report.blocker_code = Some(DiagnosticBlocker::ApplyOutcomeUncertain);
        self.report.application_accepted = None;
        self.report.application_verified = None;
        self.report.reason = format!(
            "Apply pudo haberse enviado; el resultado es incierto. Prioriza Restaurar ahora y conserva el diario. Detalle: {reason}"
        );
        let _ = self.persist_current_report();
    }

    #[cfg(test)]
    pub fn start_after_native_confirmation(
        &mut self,
        recheck: DiagnosticPreflight,
    ) -> Result<(), String> {
        self.validate_after_native_confirmation(recheck)?;
        self.arm_after_native_confirmation()?;
        self.request_diagnostic_apply()
            .map_err(|error| error.reason().to_owned())
    }
}

fn phase_for_blocker(blocker: DiagnosticBlocker) -> DiagnosticPhase {
    match blocker {
        DiagnosticBlocker::RecoveryPending => DiagnosticPhase::RecoveryPending,
        DiagnosticBlocker::ExternalConflict => DiagnosticPhase::ExternalConflict,
        DiagnosticBlocker::PreflightExpired | DiagnosticBlocker::SessionExpired => {
            DiagnosticPhase::SessionExpired
        }
        DiagnosticBlocker::WatchdogError | DiagnosticBlocker::HelperUnavailable => {
            DiagnosticPhase::WatchdogError
        }
        _ => DiagnosticPhase::Failed,
    }
}

fn confirmation_gate(
    report: &DiagnosticReport,
    client_state: Option<&str>,
    owned_preflight_journal: bool,
    recovery_pending: bool,
    expired: bool,
) -> Result<(), DiagnosticBlocker> {
    if let Some(blocker) = report.blocker_code {
        return Err(blocker);
    }
    if recovery_pending {
        return Err(DiagnosticBlocker::RecoveryPending);
    }
    if expired {
        return Err(DiagnosticBlocker::PreflightExpired);
    }
    if report.current_state_read_accepted != Some(true)
        || !report.helper_verified
        || report.helper_initialization_accepted != Some(true)
        || report.helper_state_read_accepted != Some(true)
        || report.helper_journal_durable != Some(true)
        || report.helper_same_windows_session != Some(true)
    {
        return Err(DiagnosticBlocker::WatchdogError);
    }
    if !owned_preflight_journal {
        return Err(DiagnosticBlocker::RecoveryPending);
    }
    if client_state != Some("preflight") {
        return Err(DiagnosticBlocker::WatchdogError);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service() -> DiagnosticService {
        DiagnosticService::new(std::env::temp_dir().join(format!(
            "vyra-diagnostic-{}-{}.jsonl", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        )))
    }

    #[test]
    fn failed_post_result_audit_remains_explicit_across_refresh() {
        let root = std::env::temp_dir().join(format!(
            "vyra-audit-failure-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut service = DiagnosticService::new(root.join("diagnostic-events.jsonl"));
        service.audit = crate::consent_audit::ConsentAudit::new(
            root.join("missing-parent").join("consent-audit.jsonl"),
        );
        service.report.application_accepted = Some(true);
        service.report.application_verified = Some(true);
        let error = service
            .audit_command(
                "watchdog",
                "applyAcceptedAndVerified",
                "Applying",
                "Applied",
                "fake acknowledgment",
                None,
            )
            .unwrap_err();
        service.mark_audit_persistence_failed(&error, "Watchdog informó resultado");
        let report = service.report();
        assert_eq!(report.phase, DiagnosticPhase::AuditPersistenceFailed);
        assert_eq!(
            report.blocker_code,
            Some(DiagnosticBlocker::AuditPersistenceFailed)
        );
        assert_eq!(report.application_accepted, Some(true));
        assert_eq!(report.application_verified, Some(true));
        assert_eq!(report.stage, "auditIncomplete");
        assert!(report.reason.contains("resultado durable del watchdog"));
        assert!(root.join("diagnostic-events.jsonl").exists());
        std::fs::remove_file(root.join("diagnostic-events.jsonl")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn preflight_without_watchdog_cannot_request_confirmation_and_cancel_records_no_apply() {
        let mut service = service();
        assert!(!service.path.exists()); // Construction at startup has no side effects.
        let prepared = service
            .prepare(DiagnosticPreflight::from_matrix(Ok(
                crate::color_engine::matrix_for(&crate::models::Settings::default()),
            )))
            .unwrap();
        assert_eq!(prepared.max_duration_seconds, 10);
        assert_eq!(prepared.saturation, 5);
        assert_eq!(prepared.current_state_read_accepted, Some(true));
        assert!(!prepared.can_request_confirmation);
        let cancelled = service.cancel().unwrap();
        assert_eq!(cancelled.stage, "cancelled");
        assert_eq!(cancelled.application_accepted, None);
        assert_eq!(cancelled.application_verified, None);
        assert_eq!(cancelled.restoration_accepted, None);
        let rows = std::fs::read_to_string(&service.path).unwrap();
        assert_eq!(rows.lines().count(), 2);
        std::fs::remove_file(service.path).unwrap();
    }
    #[test]
    fn preflight_error_is_visible_and_no_api_acceptance_is_invented() {
        let mut service = service();
        let report = service
            .prepare(DiagnosticPreflight::from_matrix(Err(
                "MagGetFullscreenColorEffect FALSE".into(),
            )))
            .unwrap();
        assert_eq!(report.reason, "MagGetFullscreenColorEffect FALSE");
        assert_eq!(report.application_accepted, None);
        assert_eq!(report.current_state_read_accepted, Some(false));
        assert_eq!(
            report.blocker_code,
            Some(DiagnosticBlocker::WindowsReadFailed)
        );
        std::fs::remove_file(service.path).unwrap();
    }

    fn ready_report() -> DiagnosticReport {
        DiagnosticReport {
            stage: "awaitingConfirmation".into(),
            current_state_read_accepted: Some(true),
            helper_verified: true,
            helper_initialization_accepted: Some(true),
            helper_state_read_accepted: Some(true),
            helper_journal_durable: Some(true),
            helper_same_windows_session: Some(true),
            ..DiagnosticReport::default()
        }
    }

    #[test]
    fn ready_owned_preflight_journal_allows_confirmation() {
        let report = ready_report();
        assert_eq!(
            confirmation_gate(&report, Some("preflight"), true, false, false),
            Ok(())
        );
    }

    #[test]
    fn rejected_ready_or_foreign_state_keeps_confirmation_disabled() {
        let mut report = ready_report();
        report.helper_same_windows_session = Some(false);
        assert_eq!(
            confirmation_gate(&report, Some("preflight"), true, false, false),
            Err(DiagnosticBlocker::WatchdogError)
        );
        report.helper_same_windows_session = Some(true);
        assert_eq!(
            confirmation_gate(&report, Some("armed"), true, false, false),
            Err(DiagnosticBlocker::WatchdogError)
        );
    }

    #[test]
    fn pending_recovery_and_expired_preflight_have_distinct_blockers() {
        let report = ready_report();
        assert_eq!(
            confirmation_gate(&report, Some("preflight"), false, true, false),
            Err(DiagnosticBlocker::RecoveryPending)
        );
        assert_eq!(
            confirmation_gate(&report, Some("preflight"), true, false, true),
            Err(DiagnosticBlocker::PreflightExpired)
        );
    }

    #[test]
    fn backend_mismatch_is_structured_and_confirmation_stays_blocked() {
        let mut service = service();
        let report = service
            .prepare(DiagnosticPreflight::blocked(
                DiagnosticBlocker::BackendMismatch,
                "Selecciona Windows experimental.",
            ))
            .unwrap();
        assert_eq!(
            report.blocker_code,
            Some(DiagnosticBlocker::BackendMismatch)
        );
        assert!(!report.can_request_confirmation);
        std::fs::remove_file(service.path).unwrap();
    }

    #[test]
    fn native_confirmation_failure_is_recorded_and_keeps_apply_unavailable() {
        let mut service = service();
        let report = service
            .prepare(DiagnosticPreflight::from_matrix(Ok(
                crate::color_engine::matrix_for(&crate::models::Settings::default()),
            )))
            .unwrap();
        assert!(!report.can_request_confirmation);
        let failed = service
            .fail_confirmation("Windows confirmation unavailable.")
            .unwrap();
        assert_eq!(failed.phase, DiagnosticPhase::WatchdogError);
        assert_eq!(
            failed.blocker_code,
            Some(DiagnosticBlocker::NativeConfirmationFailed)
        );
        assert!(!failed.can_request_confirmation);
        assert_eq!(failed.application_accepted, None);
        assert!(failed.reason.contains("Windows confirmation unavailable."));
        std::fs::remove_file(service.path).unwrap();
    }

    #[test]
    fn incident_lockdown_blocks_direct_start_without_consuming_recovery() {
        assert!(!crate::watchdog::REAL_DIAGNOSTIC_ENABLED);
        let mut service = service();
        let error = service
            .start_after_native_confirmation(DiagnosticPreflight::from_matrix(Ok(
                crate::color_engine::matrix_for(&crate::models::Settings::default()),
            )))
            .unwrap_err();
        assert!(error.contains("Apply bloqueado"));
        let report = service.report();
        assert_eq!(report.phase, DiagnosticPhase::ConsentIncidentLockdown);
        assert_eq!(
            report.blocker_code,
            Some(DiagnosticBlocker::ConsentIncidentLockdown)
        );
        assert!(!report.can_request_confirmation);
        assert!(!report.confirmation_accepted);
        assert_eq!(report.application_accepted, None);
        assert_eq!(report.restoration_accepted, None);
        let rows = std::fs::read_to_string(&service.path).unwrap();
        assert_eq!(rows.lines().count(), 1);
        std::fs::remove_file(service.path).unwrap();
    }

    #[test]
    fn incident_lockdown_rejects_first_and_duplicate_confirmation_requests() {
        let mut service = service();
        service.report = ready_report();
        for _ in 0..2 {
            assert!(service.begin_confirmation().is_err());
            assert!(!service.report.confirmation_accepted);
            assert!(!service.report.can_request_confirmation);
            assert_eq!(
                service.report.blocker_code,
                Some(DiagnosticBlocker::ConsentIncidentLockdown)
            );
        }
        assert!(!service.path.exists());
    }

    #[test]
    fn persisted_preflight_snapshot_includes_final_lockdown_gate() {
        let mut service = service();
        service.report = ready_report();
        let report = service.record(service.report.clone()).unwrap();
        assert_eq!(report.phase, DiagnosticPhase::ConsentIncidentLockdown);
        let persisted: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(&service.path)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(persisted["phase"], "consentIncidentLockdown");
        assert_eq!(persisted["blockerCode"], "consentIncidentLockdown");
        assert_eq!(persisted["canRequestConfirmation"], false);
        std::fs::remove_file(service.path).unwrap();
    }
}
