use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vyra-phase4-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self {
            path: path.join("recovery.json"),
        }
    }
    fn engine(&self) -> WindowsColorEngine<FakeApi> {
        WindowsColorEngine::with_api(Some(self.path.clone()), FakeApi::default())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only known fixture files; never recursively delete a computed path.
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("json.tmp"));
        let _ = fs::remove_dir(self.path.with_extension("json.tmp"));
        let _ = fs::remove_dir(self.path.parent().unwrap());
    }
}
struct FakeApi {
    matrix: Matrix,
    writes: usize,
    fail_write: bool,
    fail_cleanup: bool,
    wrong_readback: bool,
    fail_initialize: bool,
    fail_read: bool,
    partial_write_failure: bool,
}
impl Default for FakeApi {
    fn default() -> Self {
        Self {
            matrix: matrix_for(&Settings::default()),
            writes: 0,
            fail_write: false,
            fail_cleanup: false,
            wrong_readback: false,
            fail_initialize: false,
            fail_read: false,
            partial_write_failure: false,
        }
    }
}
impl MagnificationApi for FakeApi {
    fn initialize(&mut self) -> Result<(), String> {
        if self.fail_initialize {
            Err("Initialize FALSE".into())
        } else {
            Ok(())
        }
    }
    fn read(&mut self) -> Result<Matrix, String> {
        if self.fail_read {
            Err("Read FALSE".into())
        } else {
            Ok(self.matrix)
        }
    }
    fn write(&mut self, matrix: Matrix) -> Result<(), String> {
        self.writes += 1;
        if self.fail_write {
            return Err("Write FALSE".into());
        }
        if !self.wrong_readback {
            self.matrix = matrix;
        }
        if self.partial_write_failure {
            self.partial_write_failure = false;
            return Err("Partial write FALSE".into());
        }
        Ok(())
    }
    fn uninitialize(&mut self) -> Result<(), String> {
        if self.fail_cleanup {
            Err("Uninitialize FALSE".into())
        } else {
            Ok(())
        }
    }
}
fn mild() -> Settings {
    Settings {
        saturation: 5,
        ..Settings::default()
    }
}

#[test]
fn windows_effect_uses_row_major_five_by_five_transform() {
    let transform = std::array::from_fn(|i| i as f32);
    let matrix = matrix_from_transform(transform);
    assert_eq!(matrix[4], [20.0, 21.0, 22.0, 23.0, 24.0]);
    assert_eq!(transform_from_matrix(matrix), transform);
}

#[test]
fn cycle_restores_exact_original_even_after_repeated_apply_and_reset() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    let original = matrix_for(&Settings {
        contrast: 8,
        ..Settings::default()
    });
    engine.api.matrix = original;
    engine.apply(&mild()).unwrap();
    engine.apply(&mild()).unwrap();
    assert!(engine.enabled);
    assert_eq!(engine.journal.as_ref().unwrap().original, original);
    engine.reset().unwrap();
    let writes = engine.api.writes;
    engine.reset().unwrap();
    assert_eq!(engine.api.writes, writes);
    assert_eq!(engine.api.matrix, original);
    assert!(!fixture.path.exists());
    assert!(!engine.recovery_required);
}

#[test]
fn recovery_journal_round_trips_without_applying_color() {
    let fixture = Fixture::new();
    let mut first = fixture.engine();
    first.apply(&mild()).unwrap();
    let mut next = fixture.engine();
    next.api.matrix = first.api.matrix;
    assert!(next.recovery_required);
    assert!(!next.enabled);
    assert!(next.apply(&mild()).is_err());
    next.shutdown().unwrap();
    assert_eq!(next.api.writes, 0); // Startup and idle close never restore automatically.
    assert!(fixture.path.exists());
    next.reset().unwrap();
    assert_eq!(next.api.matrix, matrix_for(&Settings::default()));
    assert_eq!(next.api.writes, 1);
}

#[test]
fn invalid_recovery_journal_requires_manual_recovery() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, "broken-json").unwrap();
    let mut engine = fixture.engine();
    assert!(engine.get_current_state().recovery_required);
    assert!(engine.reset().is_err());
    assert!(engine.apply(&mild()).is_err());
    assert_eq!(engine.api.writes, 0);
    assert_eq!(fs::read_to_string(&fixture.path).unwrap(), "broken-json");
}

#[test]
fn journal_failure_prevents_first_write() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path.with_extension("json.tmp")).unwrap();
    let mut engine = fixture.engine();
    assert!(engine.apply(&mild()).unwrap_err().contains("diario"));
    assert_eq!(engine.api.writes, 0);
    assert!(!engine.enabled);
}

#[test]
fn journal_update_failure_preserves_active_original_and_can_restore() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    let bytes = fs::read(&fixture.path).unwrap();
    fs::create_dir(fixture.path.with_extension("json.tmp")).unwrap();
    assert!(engine.apply(&Settings::default()).is_err());
    assert!(engine.enabled);
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    assert_eq!(engine.api.writes, 1);
    engine.reset().unwrap();
    assert_eq!(engine.api.matrix, matrix_for(&Settings::default()));
}

#[test]
fn write_failure_rolls_back_and_reports_error() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.api.fail_write = true;
    assert!(engine.apply(&mild()).unwrap_err().contains("Write FALSE"));
    assert!(!engine.enabled);
    assert!(!engine.recovery_required);
    assert!(!fixture.path.exists()); // Read verified original; no restoration write needed.
}

#[test]
fn restore_failure_keeps_journal_and_allows_retry() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    let bytes = fs::read(&fixture.path).unwrap();
    engine.api.fail_write = true;
    assert!(engine.reset().is_err());
    assert!(engine.recovery_required);
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    assert!(engine.apply(&mild()).is_err());
    engine.api.fail_write = false;
    engine.reset().unwrap();
    assert!(!engine.recovery_required);
}

#[test]
fn accepted_write_without_matching_readback_is_not_success() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.api.wrong_readback = true;
    assert!(
        engine
            .apply(&mild())
            .unwrap_err()
            .contains("lectura no coincide")
    );
    assert!(!engine.enabled);
}

#[test]
fn external_effect_blocks_overwrite_and_keeps_evidence() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    engine.api.matrix = matrix_for(&Settings {
        hue: 20,
        ..Settings::default()
    });
    assert!(engine.reset().unwrap_err().contains("Conflicto"));
    assert_eq!(engine.api.writes, 1);
    assert!(fixture.path.exists());
}

#[test]
fn normal_close_restores_and_cleanup_failure_retains_journal() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    engine.api.fail_cleanup = true;
    assert!(engine.shutdown().is_err());
    assert!(fixture.path.exists());
    assert!(engine.recovery_required);
    engine.api.fail_cleanup = false;
    engine.shutdown().unwrap();
    assert!(!fixture.path.exists());
}

#[test]
fn active_without_original_is_blocked_without_write() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.enabled = true;
    assert!(engine.apply(&mild()).is_err());
    assert!(engine.reset().is_err());
    assert_eq!(engine.api.writes, 0);
    assert!(engine.recovery_required);
}

#[test]
fn initialize_and_read_failures_do_not_write() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.api.fail_initialize = true;
    assert!(engine.apply(&mild()).is_err());
    engine.api.fail_initialize = false;
    engine.api.fail_read = true;
    assert!(engine.apply(&mild()).is_err());
    assert_eq!(engine.api.writes, 0);
    assert!(!fixture.path.exists());
}

#[test]
fn read_only_preflight_never_captures_a_journal_or_writes() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.initialize().unwrap();
    engine.read_current().unwrap();
    assert_eq!(engine.api.writes, 0);
    assert!(engine.journal.is_none());
    assert!(!engine.initialized);
}

#[test]
fn partial_write_error_restores_original_before_reporting_failure() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.api.partial_write_failure = true;
    assert!(
        engine
            .apply(&mild())
            .unwrap_err()
            .contains("Partial write FALSE")
    );
    assert_eq!(engine.api.writes, 2);
    assert_eq!(engine.api.matrix, matrix_for(&Settings::default()));
    assert!(!engine.recovery_required);
}

#[test]
fn manager_rejects_backend_change_with_pending_recovery() {
    let fixture = Fixture::new();
    let mut active = fixture.engine();
    active.apply(&mild()).unwrap();
    let recovered = fixture.engine();
    let mut manager =
        super::super::ColorEngineManager::new(crate::models::BackendPreference::Nvidia);
    manager.windows = Box::new(recovered);
    assert!(
        manager
            .set_preference(crate::models::BackendPreference::Amd)
            .is_err()
    );
    assert!(manager.get_current_state().recovery_required);
    assert!(fixture.path.exists());
}

#[test]
fn shared_engine_mutex_serializes_concurrent_cycles() {
    use std::sync::{Arc, Mutex};
    let fixture = Fixture::new();
    let engine = Arc::new(Mutex::new(fixture.engine()));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let mut engine = engine.lock().unwrap();
                engine.apply(&mild()).unwrap();
                engine.reset().unwrap();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let engine = engine.lock().unwrap();
    assert_eq!(engine.api.writes, 16);
    assert!(!engine.enabled);
    assert!(!engine.recovery_required);
    assert!(!fixture.path.exists());
}

#[test]
fn restore_readback_mismatch_retains_original_and_journal() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    engine.api.wrong_readback = true;
    assert!(engine.reset().unwrap_err().contains("lectura no coincide"));
    assert!(engine.recovery_required);
    assert!(engine.journal.is_some());
    assert!(fixture.path.exists());
}

#[test]
fn missing_disk_journal_does_not_prevent_verified_restore_from_memory() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.apply(&mild()).unwrap();
    // Simulate external file loss only within this test's isolated temporary directory.
    fs::remove_file(&fixture.path).unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.api.matrix, matrix_for(&Settings::default()));
    assert!(!engine.enabled);
    assert!(!engine.recovery_required);
}

#[test]
fn read_only_cleanup_failure_is_retried_on_shutdown_without_color_writes() {
    let fixture = Fixture::new();
    let mut engine = fixture.engine();
    engine.api.fail_cleanup = true;
    assert!(engine.read_current().is_err());
    assert!(engine.initialized);
    assert!(
        engine
            .get_current_state()
            .description
            .contains("Uninitialize FALSE")
    );
    assert!(engine.shutdown().is_err());
    engine.api.fail_cleanup = false;
    engine.shutdown().unwrap();
    assert!(!engine.initialized);
    assert_eq!(engine.api.writes, 0);
}
