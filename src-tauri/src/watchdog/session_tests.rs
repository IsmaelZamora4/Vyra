use super::*;

#[test]
fn invalid_plans_durations_and_state_combinations_are_rejected() {
    let valid = Journal {
        version: 1,
        session: "a".repeat(64),
        parent: ParentIdentity {
            pid: 42,
            created: 1,
        },
        original: crate::color_engine::matrix_for(&Settings::default()),
        target: target(),
        max_ms: MAX_MS,
        state: State::Prepared,
        may_have_written: false,
    };
    valid.validate().unwrap();
    let mut value = valid.clone();
    value.max_ms = MAX_MS + 1;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.target[0][0] = 0.3;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.state = State::Applied;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.may_have_written = true;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.original[0][0] = f32::NAN;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.parent.created = 0;
    assert!(value.validate().is_err());
    let mut value = valid;
    value.session = "bad".into();
    assert!(value.validate().is_err());
}

#[cfg(debug_assertions)]
#[test]
fn incident_lockdown_rejects_apply_with_fake_api_and_keeps_safe_recovery() {
    use crate::watchdog::simulation::{self, Display, FileApi};

    let root = std::env::temp_dir().join(format!(
        "vyra-watchdog-consent-lockdown-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    simulation::write(&root, &Display::default()).unwrap();
    prepare(&root, &mut FileApi::new(root.clone())).unwrap();

    let mut controller = Controller::new(root.clone(), FileApi::new(root.clone())).unwrap();
    controller.prepare_read_only().unwrap();
    controller.arm_after_confirmation().unwrap();
    let journal_before = fs::read(root.join(JOURNAL)).unwrap();

    assert!(!crate::watchdog::apply_allowed(false));
    assert_eq!(simulation::read(&root).unwrap().writes, 0);
    assert_eq!(fs::read(root.join(JOURNAL)).unwrap(), journal_before);

    let outcome = controller.restore("incidentWriteLockdown");
    assert!(outcome.restoration_verified);
    assert!(!outcome.journal_retained);
    assert_eq!(simulation::read(&root).unwrap().writes, 0);
    assert!(!root.join(JOURNAL).exists());

    for name in ["mock-display.json", RESULT] {
        let path = root.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    fs::remove_dir(root).unwrap();
}

#[cfg(debug_assertions)]
#[test]
fn apply_without_confirmation_is_rejected_before_fake_api_write() {
    use crate::watchdog::simulation::{self, Display, FileApi};

    let root = std::env::temp_dir().join(format!(
        "vyra-watchdog-apply-without-confirmation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    simulation::write(&root, &Display::default()).unwrap();
    prepare(&root, &mut FileApi::new(root.clone())).unwrap();

    let mut controller = Controller::new(root.clone(), FileApi::new(root.clone())).unwrap();
    controller.prepare_read_only().unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    assert!(controller.apply(deadline).unwrap_err().contains("Armed"));
    assert_eq!(simulation::read(&root).unwrap().writes, 0);
    assert!(root.join(JOURNAL).exists());
    assert!(
        controller
            .restore("uncorroboratedConfirmation")
            .restoration_verified
    );
    assert_eq!(simulation::read(&root).unwrap().writes, 0);

    for name in ["mock-display.json", RESULT] {
        let path = root.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    fs::remove_dir(root).unwrap();
}

#[cfg(debug_assertions)]
#[test]
fn expired_deadline_after_handshake_rejects_fake_display_write() {
    use crate::watchdog::simulation::{self, Display, FileApi};

    let root = std::env::temp_dir().join(format!(
        "vyra-watchdog-expired-deadline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    simulation::write(&root, &Display::default()).unwrap();
    let journal = prepare(&root, &mut FileApi::new(root.clone())).unwrap();
    let mut controller = Controller::new(root.clone(), FileApi::new(root.clone())).unwrap();
    controller.prepare_read_only().unwrap();
    let preflight_session = controller.journal.session.clone();
    let armed_session = controller.arm_after_confirmation().unwrap();
    assert_ne!(preflight_session, armed_session);

    assert!(
        controller
            .apply(Instant::now())
            .unwrap_err()
            .contains("Plazo")
    );
    assert_eq!(simulation::read(&root).unwrap().writes, 0);
    assert!(controller.restore("test").restoration_verified);
    for name in [
        "mock-display.json",
        "watchdog-recovery.json",
        "watchdog-result.json",
    ] {
        let path = root.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    fs::remove_dir(root).unwrap();
    assert_eq!(journal.state, State::Prepared);
}

#[cfg(debug_assertions)]
#[test]
fn deadline_expiring_during_fake_read_rejects_setter() {
    use crate::color_engine::magnification::{MagnificationApi, Matrix};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct SlowReadApi {
        matrix: Matrix,
        delay: Arc<AtomicBool>,
        writes: Arc<AtomicUsize>,
    }
    impl MagnificationApi for SlowReadApi {
        fn initialize(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn read(&mut self) -> Result<Matrix, String> {
            if self.delay.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            Ok(self.matrix)
        }
        fn write(&mut self, _: Matrix) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn uninitialize(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    let root = std::env::temp_dir().join(format!(
        "vyra-watchdog-slow-read-deadline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let original = crate::color_engine::matrix_for(&Settings::default());
    let journal = Journal {
        version: 1,
        session: "b".repeat(64),
        parent: ParentIdentity::current().unwrap(),
        original,
        target: target(),
        max_ms: MAX_MS,
        state: State::Prepared,
        may_have_written: false,
    };
    crate::durable::json(&root.join(JOURNAL), &journal).unwrap();
    let delay = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicUsize::new(0));
    let api = SlowReadApi {
        matrix: original,
        delay: delay.clone(),
        writes: writes.clone(),
    };
    let mut controller = Controller::new(root.clone(), api).unwrap();
    controller.prepare_read_only().unwrap();
    controller.arm_after_confirmation().unwrap();
    delay.store(true, Ordering::Relaxed);

    let deadline = Instant::now() + std::time::Duration::from_millis(5);
    assert!(controller.apply(deadline).unwrap_err().contains("lectura"));
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    assert!(controller.restore("test").restoration_verified);
    for name in [JOURNAL, RESULT] {
        let path = root.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    fs::remove_dir(root).unwrap();
}
