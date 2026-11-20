//! Correlated, append-only consent audit records. Raw watchdog session tokens never leave memory.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeConfirmationTrace {
    pub raw_return_code: Option<i32>,
    pub classification: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditEvent {
    schema_version: u8,
    request_id: String,
    attempt_id: Option<String>,
    session_fingerprint: Option<String>,
    command: String,
    outcome: String,
    main_pid: u32,
    watchdog_pid: Option<u32>,
    timestamp_utc: String,
    previous_transition: String,
    next_transition: String,
    reason: String,
    native_confirmation: Option<NativeConfirmationTrace>,
}

pub struct ConsentAudit {
    path: PathBuf,
}

impl ConsentAudit {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        attempt_id: Option<&str>,
        session_fingerprint: Option<&str>,
        command: &str,
        outcome: &str,
        main_pid: u32,
        watchdog_pid: Option<u32>,
        previous_transition: &str,
        next_transition: &str,
        reason: &str,
        native_confirmation: Option<NativeConfirmationTrace>,
    ) -> Result<(), String> {
        let event = AuditEvent {
            schema_version: 1,
            request_id: fresh_id()?,
            attempt_id: attempt_id.map(str::to_owned),
            session_fingerprint: session_fingerprint.map(str::to_owned),
            command: command.to_owned(),
            outcome: outcome.to_owned(),
            main_pid,
            watchdog_pid,
            timestamp_utc: timestamp_utc(SystemTime::now()),
            previous_transition: previous_transition.to_owned(),
            next_transition: next_transition.to_owned(),
            reason: safe_reason(reason),
            native_confirmation,
        };
        let mut bytes = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("No se pudo abrir el diario de auditoría: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("No se pudo persistir el diario de auditoría: {error}"))
    }

    #[cfg(test)]
    pub fn record_simple(
        &self,
        attempt_id: Option<&str>,
        session_fingerprint: Option<&str>,
        command: &str,
        outcome: &str,
        watchdog_pid: Option<u32>,
        previous_transition: &str,
        next_transition: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.record(
            attempt_id,
            session_fingerprint,
            command,
            outcome,
            std::process::id(),
            watchdog_pid,
            previous_transition,
            next_transition,
            reason,
            None,
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttemptError {
    Missing,
    Stale,
    SessionMismatch,
    Expired,
    Revoked,
    Duplicate,
}

pub struct ConsentAttempt {
    id: String,
    session_fingerprint: Option<String>,
    expires_at: Option<Instant>,
    consumed: bool,
    revoked: bool,
}

impl ConsentAttempt {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            id: fresh_id()?,
            session_fingerprint: None,
            expires_at: None,
            consumed: false,
            revoked: false,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn session_fingerprint(&self) -> Option<&str> {
        self.session_fingerprint.as_deref()
    }

    pub fn bind_session(&mut self, session: &str, expires_at: Instant) {
        self.session_fingerprint = Some(fingerprint_session(session));
        self.expires_at = Some(expires_at);
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|deadline| now >= deadline)
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    pub fn claim(
        &mut self,
        requested_id: &str,
        current_session_fingerprint: Option<&str>,
        now: Instant,
    ) -> Result<(), AttemptError> {
        if requested_id != self.id {
            return Err(AttemptError::Stale);
        }
        if self.revoked {
            return Err(AttemptError::Revoked);
        }
        if self.consumed {
            return Err(AttemptError::Duplicate);
        }
        if self.expired(now) {
            self.revoked = true;
            return Err(AttemptError::Expired);
        }
        let Some(expected) = self.session_fingerprint.as_deref() else {
            return Err(AttemptError::Missing);
        };
        if current_session_fingerprint != Some(expected) {
            return Err(AttemptError::SessionMismatch);
        }
        self.consumed = true;
        Ok(())
    }
}

pub fn fingerprint_session(session: &str) -> String {
    let hash = Sha256::digest(session.as_bytes());
    let prefix: String = hash[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{prefix}")
}

fn fresh_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| format!("No se pudo generar ID: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn safe_reason(reason: &str) -> String {
    let bytes = reason.as_bytes();
    let drive_path = bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'/' | b'\\')
    });
    let unc_path = bytes.windows(2).any(|window| window == b"\\\\");
    if drive_path || unc_path {
        "Detalle operativo omitido por privacidad".into()
    } else {
        reason.chars().take(240).collect()
    }
}

fn timestamp_utc(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let days = (duration.as_secs() / 86_400) as i64;
    let day_seconds = duration.as_secs() % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    let millis = duration.subsec_millis();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_timestamp_has_rfc3339_z_suffix_and_epoch_is_exact() {
        assert_eq!(timestamp_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert!(timestamp_utc(SystemTime::now()).ends_with('Z'));
    }

    #[test]
    fn consent_attempt_is_single_use_session_bound_and_revocable() {
        let now = Instant::now();
        let mut attempt = ConsentAttempt::new().unwrap();
        attempt.bind_session("secret-ipc-session", now + Duration::from_secs(60));
        let id = attempt.id().to_owned();
        let fingerprint = attempt.session_fingerprint().unwrap().to_owned();
        assert!(!fingerprint.contains("secret-ipc-session"));
        assert_eq!(
            attempt.claim(&id, Some("sha256:wrong"), now),
            Err(AttemptError::SessionMismatch)
        );
        attempt.claim(&id, Some(&fingerprint), now).unwrap();
        assert_eq!(
            attempt.claim(&id, Some(&fingerprint), now),
            Err(AttemptError::Duplicate)
        );

        let mut expired = ConsentAttempt::new().unwrap();
        expired.bind_session("session", now - Duration::from_millis(1));
        let expired_id = expired.id().to_owned();
        let expired_session = expired.session_fingerprint().unwrap().to_owned();
        assert_eq!(
            expired.claim(&expired_id, Some(&expired_session), now),
            Err(AttemptError::Expired)
        );
        assert_eq!(
            expired.claim(&expired_id, Some(&expired_session), now),
            Err(AttemptError::Revoked)
        );
    }

    #[test]
    fn attempt_rejects_stale_missing_and_cancelled_requests() {
        let now = Instant::now();
        let mut attempt = ConsentAttempt::new().unwrap();
        attempt.bind_session("current-session", now + Duration::from_secs(1));
        let fingerprint = attempt.session_fingerprint().unwrap().to_owned();
        assert_eq!(
            attempt.claim("old-attempt", Some(&fingerprint), now),
            Err(AttemptError::Stale)
        );
        attempt.revoke();
        let attempt_id = attempt.id().to_owned();
        assert_eq!(
            attempt.claim(&attempt_id, Some(&fingerprint), now),
            Err(AttemptError::Revoked)
        );

        let mut unbound = ConsentAttempt::new().unwrap();
        let id = unbound.id().to_owned();
        assert_eq!(unbound.claim(&id, None, now), Err(AttemptError::Missing));
    }

    #[test]
    fn audit_is_append_only_correlated_and_does_not_store_ipc_session_secret() {
        let path = std::env::temp_dir().join(format!(
            "vyra-consent-audit-{}-{}.jsonl",
            std::process::id(),
            fresh_id().unwrap()
        ));
        let audit = ConsentAudit::new(path.clone());
        let fingerprint = fingerprint_session("ipc-secret");
        audit
            .record_simple(
                Some("attempt-1"),
                Some(&fingerprint),
                "start_diagnostic",
                "rejected",
                Some(42),
                "PreflightAccepted",
                "ConsentIncidentLockdown",
                "real writes disabled",
            )
            .unwrap();
        audit
            .record_simple(
                Some("attempt-1"),
                Some(&fingerprint),
                "start_diagnostic",
                "rejected",
                Some(42),
                "ConsentIncidentLockdown",
                "ConsentIncidentLockdown",
                "duplicate request",
            )
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);
        assert!(!content.contains("ipc-secret"));
        assert!(content.contains("attempt-1"));
        assert!(content.contains("\"watchdogPid\":42"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn audit_reason_redacts_absolute_windows_paths() {
        assert_eq!(
            safe_reason("No se pudo abrir C:\\Users\\Alice\\private.log"),
            "Detalle operativo omitido por privacidad"
        );
        assert_eq!(safe_reason("recovery_pending"), "recovery_pending");
    }
}
