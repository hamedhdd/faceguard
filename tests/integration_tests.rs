use faceguard_core::camera::{CameraCapture, MockCameraCapture};
use faceguard_core::config::AppConfig;
use faceguard_core::detection::{FaceDetector, FaceRecognizer, MockFaceDetector, MockFaceRecognizer};
use faceguard_core::monitoring::MonitoringController;
use faceguard_core::state::{is_valid_transition, AppState, StateTracker};
use faceguard_core::windows::{MockSessionNotifier, MockWindowsLocker, SessionNotifier, WorkstationLocker};
use std::sync::Arc;

fn create_test_fixture(
    face_present: bool,
    confidence: f32,
    threshold: u32,
    interval_sec: u64,
) -> (
    Arc<MonitoringController>,
    Arc<StateTracker>,
    Arc<MockCameraCapture>,
    Arc<MockFaceDetector>,
    Arc<MockFaceRecognizer>,
    Arc<MockWindowsLocker>,
    Arc<MockSessionNotifier>,
) {
    let mut config = AppConfig::default();
    config.missing_face_threshold = threshold;
    config.detection_interval_seconds = interval_sec;
    config.confidence_threshold = 0.70;
    config.enabled = true;
    config.lock_on_missing_face = true;

    let state_tracker = Arc::new(StateTracker::new(AppState::Monitoring));
    let camera = Arc::new(MockCameraCapture::new());
    let detector = Arc::new(MockFaceDetector::new(face_present, confidence));
    let recognizer = Arc::new(MockFaceRecognizer::new(1.0));
    let locker = Arc::new(MockWindowsLocker::new());
    let notifier = Arc::new(MockSessionNotifier::new());

    let controller = MonitoringController::new(
        config,
        Arc::clone(&state_tracker),
        Arc::clone(&camera) as Arc<dyn CameraCapture>,
        Arc::clone(&detector) as Arc<dyn FaceDetector>,
        Arc::clone(&recognizer) as Arc<dyn FaceRecognizer>,
        Arc::clone(&locker) as Arc<dyn WorkstationLocker>,
        Arc::clone(&notifier) as Arc<dyn SessionNotifier>,
    );

    (controller, state_tracker, camera, detector, recognizer, locker, notifier)
}

#[test]
fn test_tract_face_recognition_loading() {
    use tract_onnx::prelude::*;
    let model_path = std::path::PathBuf::from("models/face_recognition.onnx");
    if model_path.exists() {
        let model = tract_onnx::onnx()
            .model_for_path(&model_path)
            .expect("Failed to read ONNX model")
            .with_input_fact(0, f32::fact([1, 3, 112, 112]).into())
            .expect("Failed to set input fact")
            .into_optimized()
            .expect("Failed to optimize model")
            .into_runnable()
            .expect("Failed to make model runnable");

        let tract_tensor = Tensor::zero::<f32>(&[1, 3, 112, 112]).unwrap();
        let result = model.run(tvec!(tract_tensor.into())).expect("Model inference failed");
        assert_eq!(result.len(), 1);
        let out_view = result[0].to_array_view::<f32>().expect("Failed to view output tensor");
        assert_eq!(out_view.shape(), &[1, 512]);
        println!("Face recognition (MobileFaceNet 512-D) inference verified! Output shape: {:?}", out_view.shape());
    }
}

#[test]
fn test_state_machine_transitions() {
    let tracker = StateTracker::new(AppState::Initializing);
    assert_eq!(tracker.current(), AppState::Initializing);

    // Valid transition: Initializing -> Monitoring
    assert!(tracker.transition(AppState::Monitoring).unwrap());
    assert_eq!(tracker.current(), AppState::Monitoring);

    // Valid transition: Monitoring -> FaceDetected
    assert!(tracker.transition(AppState::FaceDetected).unwrap());

    // Valid transition: FaceDetected -> Monitoring
    assert!(tracker.transition(AppState::Monitoring).unwrap());

    // Valid transition: Monitoring -> FaceMissing
    assert!(tracker.transition(AppState::FaceMissing).unwrap());

    // Valid transition: FaceMissing -> LockRequested
    assert!(tracker.transition(AppState::LockRequested).unwrap());

    // Valid transition: LockRequested -> LockedStandby
    assert!(tracker.transition(AppState::LockedStandby).unwrap());

    // Valid transition: LockedStandby -> Monitoring
    assert!(tracker.transition(AppState::Monitoring).unwrap());

    // Valid transition: Monitoring -> Disabled
    assert!(tracker.transition(AppState::Disabled).unwrap());

    // Valid transition: Disabled -> Monitoring
    assert!(tracker.transition(AppState::Monitoring).unwrap());

    // Valid transition to ShuttingDown from anywhere
    assert!(tracker.transition(AppState::ShuttingDown).unwrap());

    // Invalid transition checks
    assert!(!is_valid_transition(AppState::LockedStandby, AppState::FaceDetected));
    assert!(!is_valid_transition(AppState::Disabled, AppState::LockedStandby));
    assert!(!is_valid_transition(AppState::ShuttingDown, AppState::Monitoring));
}

#[test]
fn test_missing_face_threshold_logic() {
    // Threshold = 2 failed checks before lock
    let (controller, _state, _cam, detector, _rec, locker, _notifier) =
        create_test_fixture(false, 0.20, 2, 20);

    assert_eq!(locker.lock_count(), 0);

    // Check 1: Face absent -> missing counter becomes 1, no lock
    let res1 = controller.perform_check().unwrap();
    assert!(!res1.is_face_present);
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 1);
    assert_eq!(locker.lock_count(), 0);

    // Simulate face reappearing briefly!
    detector.set_face_present(true);
    detector.set_confidence(0.95);

    let res2 = controller.perform_check().unwrap();
    assert!(res2.is_face_present);
    // Counter MUST reset to 0!
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 0);
    assert_eq!(locker.lock_count(), 0);

    // Now face disappears again
    detector.set_face_present(false);
    detector.set_confidence(0.10);

    // Failed check 1
    let _ = controller.perform_check().unwrap();
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 1);
    assert_eq!(locker.lock_count(), 0);

    // Failed check 2 -> Threshold reached! Lock should be invoked!
    let _ = controller.perform_check().unwrap();
    assert_eq!(locker.lock_count(), 1);
    assert_eq!(controller.get_telemetry().state, AppState::LockedStandby);
}

#[test]
fn test_camera_error_handling() {
    let (controller, _state, camera, _detector, _rec, locker, _notifier) =
        create_test_fixture(false, 0.0, 2, 20);

    // Simulate camera hardware disconnect or occupancy error
    camera.set_failure(Some("Webcam busy or disconnected".to_string()));

    let res = controller.perform_check();
    assert!(res.is_err());
    assert_eq!(controller.get_telemetry().state, AppState::CameraError);

    // CRITICAL: Camera error must NOT increment missing-face counter!
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 0);

    // CRITICAL: Camera error must NOT lock Windows workstation!
    assert_eq!(locker.lock_count(), 0);

    // Camera recovers
    camera.set_failure(None);
    // Perform check should recover state
    let _ = controller.perform_check();
    assert_ne!(controller.get_telemetry().state, AppState::CameraError);
}

#[test]
fn test_kill_switch_behavior() {
    let (controller, _state, _cam, _detector, _rec, locker, _notifier) =
        create_test_fixture(false, 0.0, 1, 20);

    // Activate Kill Switch
    controller.disable_monitoring();
    assert_eq!(controller.get_telemetry().state, AppState::Disabled);
    assert!(!controller.get_telemetry().enabled);

    // Performing checks while Disabled must be skipped
    let check_res = controller.perform_check();
    assert!(check_res.is_err());
    assert_eq!(locker.lock_count(), 0);

    // Resume Monitoring
    controller.resume_monitoring();
    assert_eq!(controller.get_telemetry().state, AppState::Monitoring);
    assert!(controller.get_telemetry().enabled);
}

#[test]
fn test_scheduler_cancellation() {
    let (controller, _state, _cam, _detector, _rec, _locker, _notifier) =
        create_test_fixture(true, 0.90, 2, 1);

    controller.start_scheduler_loop();

    // Small delay to let scheduler start
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Cancel via shutdown/disable
    controller.disable_monitoring();

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(controller.get_telemetry().state, AppState::Disabled);
}

#[test]
fn test_windows_lock_request_abstraction() {
    let (controller, _state, _cam, _detector, _rec, locker, _notifier) =
        create_test_fixture(true, 0.90, 2, 20);

    assert_eq!(locker.lock_count(), 0);

    // Trigger manual lock
    let res = controller.lock_now();
    assert!(res.is_ok());
    assert_eq!(locker.lock_count(), 1);
    assert_eq!(controller.get_telemetry().state, AppState::LockedStandby);
}

#[test]
fn test_locked_standby_behavior() {
    let (controller, state, _cam, _detector, _rec, _locker, _notifier) =
        create_test_fixture(true, 0.90, 2, 20);

    state.force_set(AppState::LockedStandby);
    assert_eq!(controller.get_telemetry().state, AppState::LockedStandby);

    // In LockedStandby, perform_check is completely rejected
    let res = controller.perform_check();
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Locked Standby"));
}

#[test]
fn test_resume_after_session_unlock() {
    let (controller, state, _cam, detector, _rec, _locker, notifier) =
        create_test_fixture(true, 0.95, 2, 20);

    // Simulate Windows workstation lock
    notifier.simulate_lock();
    assert_eq!(state.current(), AppState::LockedStandby);

    // Simulate user returning and unlocking Windows
    detector.set_face_present(true);
    detector.set_confidence(0.92);
    notifier.simulate_unlock();

    // Controller should resume monitoring and execute presence check
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 0);
    assert_eq!(state.current(), AppState::Monitoring);
}

#[test]
fn test_face_recognition_owner_match() {
    let (controller, _state, _cam, detector, recognizer, locker, _notifier) =
        create_test_fixture(true, 0.95, 2, 20);

    // Configure enrolled owner face
    {
        let mut cfg = controller.config.write();
        cfg.enrolled_face = Some(vec![0.1; 512]);
        cfg.recognition_enabled = true;
        cfg.recognition_threshold = 0.60;
    }

    // Owner face is in front of camera with high similarity (0.92)
    detector.set_face_present(true);
    recognizer.set_similarity(0.92);

    let res = controller.perform_check().unwrap();
    assert!(res.is_face_present);
    let rec = res.recognition.unwrap();
    assert!(rec.is_recognized);
    assert!((rec.similarity_score - 0.92).abs() < 1e-4);

    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 0);
    assert_eq!(locker.lock_count(), 0);
}

#[test]
fn test_face_recognition_stranger_lock() {
    let (controller, _state, _cam, detector, recognizer, locker, _notifier) =
        create_test_fixture(true, 0.95, 2, 20);

    // Configure enrolled owner face
    {
        let mut cfg = controller.config.write();
        cfg.enrolled_face = Some(vec![0.1; 512]);
        cfg.recognition_enabled = true;
        cfg.recognition_threshold = 0.60;
    }

    // Stranger face in front of camera: face IS present (95%), but similarity is low (0.25 < 0.60)
    detector.set_face_present(true);
    recognizer.set_similarity(0.25);

    // Check 1: Stranger detected -> treated as unauthorized absence
    let res1 = controller.perform_check().unwrap();
    assert!(res1.is_face_present);
    let rec1 = res1.recognition.unwrap();
    assert!(!rec1.is_recognized);
    assert_eq!(controller.get_telemetry().consecutive_failed_checks, 1);
    assert_eq!(locker.lock_count(), 0);

    // Check 2: Stranger still there -> threshold 2 reached -> Lock Windows!
    let _ = controller.perform_check().unwrap();
    assert_eq!(locker.lock_count(), 1);
    assert_eq!(controller.get_telemetry().state, AppState::LockedStandby);
}

#[test]
fn test_face_enrollment_and_clear() {
    let (controller, _state, _cam, detector, _rec, _locker, _notifier) =
        create_test_fixture(true, 0.90, 2, 20);

    // Face present -> enroll should succeed
    detector.set_face_present(true);
    let enroll_res = controller.enroll_current_face();
    assert!(enroll_res.is_ok());

    assert!(controller.config.read().enrolled_face.is_some());
    assert!(controller.config.read().recognition_enabled);

    // Now clear enrollment
    let clear_res = controller.clear_enrolled_face();
    assert!(clear_res.is_ok());
    assert!(controller.config.read().enrolled_face.is_none());
    assert!(!controller.config.read().recognition_enabled);
}

#[test]
fn test_config_validation() {
    let mut config = AppConfig {
        enabled: true,
        detection_interval_seconds: 1, // Invalid low (<3)
        missing_face_threshold: 0,     // Invalid low (<1)
        confidence_threshold: 1.5,     // Invalid high (>0.99)
        minimum_face_size: 10,         // Invalid low (<16)
        camera_index: 0,
        start_with_windows: false,
        minimize_to_tray: true,
        notifications_enabled: true,
        lock_on_missing_face: true,
        grace_period_seconds: 500,     // Invalid high (>300)
        recognition_enabled: false,
        recognition_threshold: 1.5,    // Invalid high (>0.99)
        enrolled_face: None,
    };

    config.validate();

    assert_eq!(config.detection_interval_seconds, 3);
    assert_eq!(config.missing_face_threshold, 1);
    assert_eq!(config.confidence_threshold, 0.99);
    assert_eq!(config.minimum_face_size, 16);
    assert_eq!(config.grace_period_seconds, 300);
    assert_eq!(config.recognition_threshold, 0.99);

    // Test JSON serialization & deserialization
    let json = serde_json::to_string(&config).unwrap();
    let parsed: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, parsed);
}
