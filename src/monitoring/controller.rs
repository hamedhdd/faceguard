use crate::camera::CameraCapture;
use crate::config::AppConfig;
use crate::detection::{DetectionSummary, FaceDetector, FaceRecognizer, RecognitionSummary};
use crate::state::{AppState, StateTracker};
use crate::windows::{SessionEvent, SessionNotifier, WorkstationLocker};
use chrono::{DateTime, Local};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub state: AppState,
    pub enabled: bool,
    pub camera_index: u32,
    pub camera_name: String,
    pub consecutive_failed_checks: u32,
    pub threshold: u32,
    pub interval_seconds: u64,
    pub confidence_threshold: f32,
    pub last_confidence: f32,
    pub last_check_time: Option<DateTime<Local>>,
    pub last_detection_time: Option<DateTime<Local>>,
    pub next_check_time: Option<DateTime<Local>>,
    pub last_error: Option<String>,
    pub total_checks: u64,
    pub total_locks: u64,
    // Face Recognition fields
    pub recognition_enabled: bool,
    pub recognition_threshold: f32,
    pub is_enrolled: bool,
    pub last_similarity: f32,
    pub last_recognized: Option<bool>,
}

pub struct MonitoringController {
    pub config: RwLock<AppConfig>,
    pub state_tracker: Arc<StateTracker>,
    camera: Arc<dyn CameraCapture>,
    detector: Arc<dyn FaceDetector>,
    recognizer: Arc<dyn FaceRecognizer>,
    locker: Arc<dyn WorkstationLocker>,
    session_notifier: Arc<dyn SessionNotifier>,

    // Operational counters and timestamps
    failed_checks_counter: AtomicU32,
    total_checks_counter: AtomicU64,
    total_locks_counter: AtomicU64,
    last_confidence: RwLock<f32>,
    last_similarity: RwLock<f32>,
    last_recognized: RwLock<Option<bool>>,
    last_check_time: RwLock<Option<DateTime<Local>>>,
    last_detection_time: RwLock<Option<DateTime<Local>>>,
    next_check_time: RwLock<Option<DateTime<Local>>>,
    last_error_message: RwLock<Option<String>>,
    active_camera_name: RwLock<String>,

    // Cancellation & Control
    is_scheduler_running: AtomicBool,
    kill_switch_active: AtomicBool,
    cancel_sender: RwLock<Option<crossbeam_channel::Sender<()>>>,
}

impl MonitoringController {
    pub fn new(
        config: AppConfig,
        state_tracker: Arc<StateTracker>,
        camera: Arc<dyn CameraCapture>,
        detector: Arc<dyn FaceDetector>,
        recognizer: Arc<dyn FaceRecognizer>,
        locker: Arc<dyn WorkstationLocker>,
        session_notifier: Arc<dyn SessionNotifier>,
    ) -> Arc<Self> {
        let is_enabled = config.enabled;
        let controller = Arc::new(Self {
            config: RwLock::new(config),
            state_tracker,
            camera,
            detector,
            recognizer,
            locker,
            session_notifier,
            failed_checks_counter: AtomicU32::new(0),
            total_checks_counter: AtomicU64::new(0),
            total_locks_counter: AtomicU64::new(0),
            last_confidence: RwLock::new(0.0),
            last_similarity: RwLock::new(0.0),
            last_recognized: RwLock::new(None),
            last_check_time: RwLock::new(None),
            last_detection_time: RwLock::new(None),
            next_check_time: RwLock::new(None),
            last_error_message: RwLock::new(None),
            active_camera_name: RwLock::new("Default Camera".to_string()),
            is_scheduler_running: AtomicBool::new(false),
            kill_switch_active: AtomicBool::new(!is_enabled),
            cancel_sender: RwLock::new(None),
        });

        // Initialize camera device name query
        controller.refresh_camera_name();

        // Subscribe to session lock/unlock events
        let weak_controller = Arc::downgrade(&controller);
        let _ = controller.session_notifier.start(Arc::new(move |event| {
            if let Some(ctrl) = weak_controller.upgrade() {
                ctrl.handle_session_event(event);
            }
        }));

        controller
    }

    pub fn refresh_camera_name(&self) {
        let cam_idx = self.config.read().camera_index;
        if let Ok(cams) = self.camera.list_cameras() {
            if let Some(c) = cams.into_iter().find(|c| c.index == cam_idx) {
                *self.active_camera_name.write() = c.name;
                return;
            }
        }
        *self.active_camera_name.write() = format!("Camera #{cam_idx}");
    }

    /// Handles Windows session lock and unlock events.
    pub fn handle_session_event(&self, event: SessionEvent) {
        match event {
            SessionEvent::Locked => {
                log::info!("Session event: Windows session locked");
                let cur = self.state_tracker.current();
                if cur != AppState::LockedStandby && cur != AppState::Disabled {
                    let _ = self.state_tracker.transition(AppState::LockedStandby);
                }
            }
            SessionEvent::Unlocked => {
                log::info!("Session event: Windows session unlocked");
                let cur = self.state_tracker.current();
                if cur == AppState::LockedStandby {
                    // Reset missing-face counter and resume monitoring
                    self.failed_checks_counter.store(0, Ordering::Relaxed);
                    *self.last_error_message.write() = None;

                    if !self.kill_switch_active.load(Ordering::Relaxed) {
                        log::info!("Resuming face presence monitoring after session unlock...");
                        let _ = self.state_tracker.transition(AppState::Monitoring);
                        // Trigger immediate verification check on unlock
                        let _ = self.perform_check();
                    } else {
                        log::info!("Kill switch is active; remaining in Disabled state");
                        let _ = self.state_tracker.transition(AppState::Disabled);
                    }
                }
            }
        }
    }

    /// Performs one discrete face presence and identity verification check.
    /// Captures a single frame, runs detection and optional recognition,
    /// releases frame immediately, and evaluates absence/unauthorized policy.
    pub fn perform_check(&self) -> Result<DetectionSummary, String> {
        let cur_state = self.state_tracker.current();
        if cur_state == AppState::LockedStandby || cur_state == AppState::Disabled || cur_state == AppState::ShuttingDown {
            return Err(format!("Check skipped: current state is {cur_state}"));
        }

        self.total_checks_counter.fetch_add(1, Ordering::Relaxed);
        let now = Local::now();
        *self.last_check_time.write() = Some(now);

        let (cam_idx, conf_thresh, min_size, missing_thresh, do_lock, rec_enabled, rec_thresh, enrolled_opt) = {
            let c = self.config.read();
            (
                c.camera_index,
                c.confidence_threshold,
                c.minimum_face_size,
                c.missing_face_threshold,
                c.lock_on_missing_face,
                c.recognition_enabled,
                c.recognition_threshold,
                c.enrolled_face.clone(),
            )
        };

        log::debug!("Starting presence/recognition check on camera {cam_idx}...");

        // 1. Capture single frame
        let frame = match self.camera.capture_single_frame(cam_idx) {
            Ok(f) => {
                *self.last_error_message.write() = None;
                f
            }
            Err(e) => {
                let err_str = e.to_string();
                log::error!("Camera capture error during check: {err_str}");
                *self.last_error_message.write() = Some(err_str.clone());
                // CRITICAL REQUIREMENT: Camera errors must NOT trigger a lock or increment missing_face_counter!
                let _ = self.state_tracker.transition(AppState::CameraError);
                return Err(err_str);
            }
        };

        // 2. Run face detection inference
        let mut summary = match self.detector.detect(&frame, conf_thresh, min_size) {
            Ok(s) => s,
            Err(e) => {
                let err_str = e.to_string();
                log::error!("Face detection model error: {err_str}");
                *self.last_error_message.write() = Some(err_str.clone());
                let _ = self.state_tracker.transition(AppState::CameraError);
                return Err(err_str);
            }
        };

        *self.last_confidence.write() = summary.max_confidence;

        // 3. Evaluate identity recognition if enabled and enrolled
        let mut is_authorized = summary.is_face_present;

        if summary.is_face_present {
            if let Some(ref face) = summary.primary_face {
                if rec_enabled && enrolled_opt.is_some() {
                    let target_emb = enrolled_opt.as_ref().unwrap();
                    match self.recognizer.extract_embedding(&frame, face) {
                        Ok(current_emb) => {
                            let sim = self.recognizer.compute_similarity(&current_emb, target_emb);
                            let matched = sim >= rec_thresh;
                            *self.last_similarity.write() = sim;
                            *self.last_recognized.write() = Some(matched);

                            summary.recognition = Some(RecognitionSummary {
                                is_recognized: matched,
                                similarity_score: sim,
                                threshold: rec_thresh,
                                is_enrolled: true,
                            });

                            if matched {
                                log::info!(
                                    "Owner face recognized: similarity {:.1}% >= threshold {:.1}%",
                                    sim * 100.0,
                                    rec_thresh * 100.0
                                );
                                is_authorized = true;
                            } else {
                                log::warn!(
                                    "Unrecognized face detected: similarity {:.1}% < threshold {:.1}% (Owner absent)",
                                    sim * 100.0,
                                    rec_thresh * 100.0
                                );
                                is_authorized = false;
                            }
                        }
                        Err(e) => {
                            log::error!("Face recognition embedding error: {e}");
                            is_authorized = false;
                        }
                    }
                } else {
                    // Face presence mode: face detected is sufficient
                    *self.last_similarity.write() = 0.0;
                    *self.last_recognized.write() = None;
                }
            }
        } else {
            *self.last_similarity.write() = 0.0;
            *self.last_recognized.write() = None;
        }

        // Explicitly drop captured frame immediately after processing
        drop(frame);

        // 4. Evaluate presence / authorization state
        if is_authorized {
            // Authorized owner confirmed! Reset absence counter
            self.failed_checks_counter.store(0, Ordering::Relaxed);
            *self.last_detection_time.write() = Some(summary.timestamp);

            log::info!(
                "Presence authorized: face detected with {:.1}% confidence (inference: {:.1}ms)",
                summary.max_confidence * 100.0,
                summary.inference_duration_ms
            );

            let _ = self.state_tracker.transition(AppState::FaceDetected);
            let _ = self.state_tracker.transition(AppState::Monitoring);
        } else {
            // Face missing or unrecognized stranger!
            let failed = self.failed_checks_counter.fetch_add(1, Ordering::Relaxed) + 1;
            log::warn!(
                "Authorized user absent in frame! Failed checks: {}/{}",
                failed,
                missing_thresh
            );

            let _ = self.state_tracker.transition(AppState::FaceMissing);

            // Check if absence threshold reached
            if failed >= missing_thresh {
                log::warn!(
                    "Missing-face threshold reached ({failed}/{missing_thresh}). Initiating Windows lock..."
                );

                if do_lock {
                    let _ = self.state_tracker.transition(AppState::LockRequested);
                    self.total_locks_counter.fetch_add(1, Ordering::Relaxed);

                    match self.locker.lock_workstation() {
                        Ok(_) => {
                            log::info!("LockWorkStation succeeded. Entering LockedStandby state.");
                            let _ = self.state_tracker.transition(AppState::LockedStandby);
                        }
                        Err(e) => {
                            log::error!("Failed to lock workstation: {e}");
                            *self.last_error_message.write() = Some(e.to_string());
                            let _ = self.state_tracker.transition(AppState::Monitoring);
                        }
                    }
                } else {
                    log::info!("lock_on_missing_face is disabled in settings; workstation not locked.");
                }
            }
        }

        Ok(summary)
    }

    /// Enrolls the face currently visible to the camera as the authorized owner.
    pub fn enroll_current_face(&self) -> Result<f32, String> {
        let (cam_idx, conf_thresh, min_size) = {
            let c = self.config.read();
            (c.camera_index, c.confidence_threshold, c.minimum_face_size)
        };

        log::info!("Capturing frame for owner face enrollment...");
        let frame = self.camera.capture_single_frame(cam_idx).map_err(|e| e.to_string())?;
        let summary = self.detector.detect(&frame, conf_thresh, min_size).map_err(|e| e.to_string())?;

        if !summary.is_face_present || summary.primary_face.is_none() {
            return Err("No face detected in camera frame. Please look directly at the webcam with good lighting.".to_string());
        }

        let face = summary.primary_face.as_ref().unwrap();
        let embedding = self.recognizer.extract_embedding(&frame, face).map_err(|e| e.to_string())?;

        {
            let mut cfg = self.config.write();
            cfg.enrolled_face = Some(embedding);
            cfg.recognition_enabled = true;
            let _ = cfg.save();
        }

        *self.last_similarity.write() = 1.0;
        *self.last_recognized.write() = Some(true);

        log::info!("Owner face enrolled successfully! 512-D embedding saved.");
        Ok(summary.max_confidence)
    }

    /// Clears the enrolled owner face profile.
    pub fn clear_enrolled_face(&self) -> Result<(), String> {
        {
            let mut cfg = self.config.write();
            cfg.enrolled_face = None;
            cfg.recognition_enabled = false;
            let _ = cfg.save();
        }
        *self.last_similarity.write() = 0.0;
        *self.last_recognized.write() = None;
        log::info!("Enrolled face profile cleared. System reverted to general presence mode.");
        Ok(())
    }

    /// Tests face recognition on a live frame and returns the similarity score.
    pub fn test_recognition(&self) -> Result<RecognitionSummary, String> {
        let (cam_idx, conf_thresh, min_size, rec_thresh, enrolled_opt) = {
            let c = self.config.read();
            (
                c.camera_index,
                c.confidence_threshold,
                c.minimum_face_size,
                c.recognition_threshold,
                c.enrolled_face.clone(),
            )
        };

        let frame = self.camera.capture_single_frame(cam_idx).map_err(|e| e.to_string())?;
        let summary = self.detector.detect(&frame, conf_thresh, min_size).map_err(|e| e.to_string())?;

        if !summary.is_face_present || summary.primary_face.is_none() {
            return Err("No face detected in test frame.".to_string());
        }

        let face = summary.primary_face.as_ref().unwrap();
        let embedding = self.recognizer.extract_embedding(&frame, face).map_err(|e| e.to_string())?;

        if let Some(target) = enrolled_opt {
            let sim = self.recognizer.compute_similarity(&embedding, &target);
            let matched = sim >= rec_thresh;
            *self.last_similarity.write() = sim;
            *self.last_recognized.write() = Some(matched);

            Ok(RecognitionSummary {
                is_recognized: matched,
                similarity_score: sim,
                threshold: rec_thresh,
                is_enrolled: true,
            })
        } else {
            Ok(RecognitionSummary {
                is_recognized: false,
                similarity_score: 0.0,
                threshold: rec_thresh,
                is_enrolled: false,
            })
        }
    }

    /// Manual workstation lock request.
    pub fn lock_now(&self) -> Result<(), String> {
        log::info!("Manual lock requested by user");
        let _ = self.state_tracker.transition(AppState::LockRequested);
        self.total_locks_counter.fetch_add(1, Ordering::Relaxed);

        self.locker.lock_workstation()
            .map_err(|e| e.to_string())?;

        let _ = self.state_tracker.transition(AppState::LockedStandby);
        Ok(())
    }

    /// Kill switch / Emergency Stop.
    pub fn disable_monitoring(&self) {
        log::warn!("Kill switch activated: stopping monitoring immediately");
        self.kill_switch_active.store(true, Ordering::SeqCst);

        // Cancel running scheduler cycle
        if let Some(sender) = self.cancel_sender.read().as_ref() {
            let _ = sender.send(());
        }

        self.failed_checks_counter.store(0, Ordering::Relaxed);
        let _ = self.state_tracker.transition(AppState::Disabled);
    }

    /// Resume monitoring.
    pub fn resume_monitoring(self: &Arc<Self>) {
        log::info!("Resuming face monitoring...");
        self.kill_switch_active.store(false, Ordering::SeqCst);
        self.failed_checks_counter.store(0, Ordering::Relaxed);
        *self.last_error_message.write() = None;

        let _ = self.state_tracker.transition(AppState::Monitoring);

        // Start scheduler loop if not already running
        self.start_scheduler_loop();
    }

    /// Starts the background scheduler thread.
    pub fn start_scheduler_loop(self: &Arc<Self>) {
        if self.is_scheduler_running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let (tx, rx) = crossbeam_channel::bounded(1);
        *self.cancel_sender.write() = Some(tx);

        let controller = Arc::clone(self);

        std::thread::Builder::new()
            .name("faceguard-scheduler".to_string())
            .spawn(move || {
                log::info!("Monitoring scheduler thread started");

                // Perform an initial check shortly after startup if active
                std::thread::sleep(Duration::from_millis(500));
                if controller.state_tracker.current() == AppState::Monitoring {
                    let _ = controller.perform_check();
                }

                loop {
                    let interval_sec = controller.config.read().detection_interval_seconds;
                    let next_time = Local::now() + chrono::Duration::seconds(interval_sec as i64);
                    *controller.next_check_time.write() = Some(next_time);

                    // Wait for interval OR cancellation signal
                    match rx.recv_timeout(Duration::from_secs(interval_sec)) {
                        Ok(()) => {
                            log::info!("Scheduler received cancellation signal");
                            break;
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                            let state = controller.state_tracker.current();
                            if state == AppState::ShuttingDown {
                                break;
                            }

                            // If state is Monitoring or FaceDetected or FaceMissing, run scheduled check
                            if state == AppState::Monitoring
                                || state == AppState::FaceDetected
                                || state == AppState::FaceMissing
                                || state == AppState::CameraError
                            {
                                let _ = controller.perform_check();
                            }
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                            log::info!("Scheduler channel disconnected");
                            break;
                        }
                    }
                }

                controller.is_scheduler_running.store(false, Ordering::SeqCst);
                log::info!("Monitoring scheduler thread terminated");
            })
            .expect("Failed to spawn scheduler thread");
    }

    /// Returns a full snapshot of current telemetry for the UI dashboard.
    pub fn get_telemetry(&self) -> TelemetrySnapshot {
        let (interval, thresh, conf_thresh, cam_idx, rec_enabled, rec_thresh, is_enrolled) = {
            let c = self.config.read();
            (
                c.detection_interval_seconds,
                c.missing_face_threshold,
                c.confidence_threshold,
                c.camera_index,
                c.recognition_enabled,
                c.recognition_threshold,
                c.enrolled_face.is_some(),
            )
        };

        TelemetrySnapshot {
            state: self.state_tracker.current(),
            enabled: !self.kill_switch_active.load(Ordering::Relaxed),
            camera_index: cam_idx,
            camera_name: self.active_camera_name.read().clone(),
            consecutive_failed_checks: self.failed_checks_counter.load(Ordering::Relaxed),
            threshold: thresh,
            interval_seconds: interval,
            confidence_threshold: conf_thresh,
            last_confidence: *self.last_confidence.read(),
            last_check_time: *self.last_check_time.read(),
            last_detection_time: *self.last_detection_time.read(),
            next_check_time: *self.next_check_time.read(),
            last_error: self.last_error_message.read().clone(),
            total_checks: self.total_checks_counter.load(Ordering::Relaxed),
            total_locks: self.total_locks_counter.load(Ordering::Relaxed),
            recognition_enabled: rec_enabled,
            recognition_threshold: rec_thresh,
            is_enrolled,
            last_similarity: *self.last_similarity.read(),
            last_recognized: *self.last_recognized.read(),
        }
    }

    /// Updates configuration settings in runtime and persists to disk.
    pub fn update_config(&self, new_config: AppConfig) -> Result<(), String> {
        let mut cfg = new_config;
        cfg.validate();

        if cfg.save().is_err() {
            log::warn!("Could not persist config to disk");
        }

        let old_cam = self.config.read().camera_index;
        let new_cam = cfg.camera_index;

        *self.config.write() = cfg;

        if old_cam != new_cam {
            self.refresh_camera_name();
        }

        Ok(())
    }

    /// Clean shutdown of controller and background threads.
    pub fn shutdown(&self) {
        log::info!("Shutting down MonitoringController...");
        let _ = self.state_tracker.transition(AppState::ShuttingDown);
        if let Some(sender) = self.cancel_sender.read().as_ref() {
            let _ = sender.send(());
        }
        self.session_notifier.stop();
    }
}
