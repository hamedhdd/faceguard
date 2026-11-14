// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use faceguard_core::app::{self, AppContainer};
use faceguard_core::camera::{self, NokhwaCameraCapture};
use faceguard_core::config::AppConfig;
use faceguard_core::detection::{self, OnnxFaceDetector};
use faceguard_core::monitoring::MonitoringController;
use faceguard_core::state::{AppState, StateTracker};
use faceguard_core::ui;
use faceguard_core::windows::{NativeWindowsLocker, WindowsSessionNotifier};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, WindowEvent};

fn resolve_model_path() -> PathBuf {
    // 1. Check relative to current working directory
    let cwd_path = PathBuf::from("models/face_detection.onnx");
    if cwd_path.exists() {
        return cwd_path;
    }

    // 2. Check next to current executable
    if let Ok(exe_dir) = std::env::current_exe() {
        if let Some(parent) = exe_dir.parent() {
            let exe_model = parent.join("models").join("face_detection.onnx");
            if exe_model.exists() {
                return exe_model;
            }
            let direct_model = parent.join("face_detection.onnx");
            if direct_model.exists() {
                return direct_model;
            }
        }
    }

    // 3. Fallback to models/face_detection_yunet.onnx
    let yunet_path = PathBuf::from("models/face_detection_yunet.onnx");
    if yunet_path.exists() {
        return yunet_path;
    }

    PathBuf::from("models/face_detection.onnx")
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting FaceGuard v1.0.0...");

    let mut config = AppConfig::load();
    config.validate();

    let state_tracker = Arc::new(StateTracker::new(AppState::Initializing));
    let camera_backend = Arc::new(NokhwaCameraCapture::new());

    let model_path = resolve_model_path();
    log::info!("Loading face detection model from {:?}", model_path);

    let detector = match OnnxFaceDetector::from_file(&model_path) {
        Ok(d) => Arc::new(d) as Arc<dyn detection::FaceDetector>,
        Err(e) => {
            log::error!("Failed to load ONNX model: {e}");
            panic!("Could not load face detection model: {e}");
        }
    };

    let locker = Arc::new(NativeWindowsLocker::new());
    let session_notifier = Arc::new(WindowsSessionNotifier::new());

    let recognizer = match detection::OnnxFaceRecognizer::new(None as Option<&str>) {
        Ok(r) => {
            log::info!("Face recognition engine initialized successfully.");
            Arc::new(r) as Arc<dyn detection::FaceRecognizer>
        }
        Err(e) => {
            log::warn!("Could not load face recognition model ({e}), falling back to mock recognizer");
            Arc::new(detection::MockFaceRecognizer::new(0.0)) as Arc<dyn detection::FaceRecognizer>
        }
    };

    let controller = MonitoringController::new(
        config.clone(),
        Arc::clone(&state_tracker),
        Arc::clone(&camera_backend) as Arc<dyn camera::CameraCapture>,
        detector,
        recognizer,
        locker,
        session_notifier,
    );

    let app_container = AppContainer {
        controller: Arc::clone(&controller),
        camera_backend,
    };

    let controller_for_setup = Arc::clone(&controller);
    let state_tracker_for_tray = Arc::clone(&state_tracker);

    let launch_minimized = std::env::args().any(|arg| arg == "--minimized") || !config.enabled;

    tauri::Builder::default()
        .manage(app_container)
        .setup(move |app| {
            // Setup system tray
            let tray = match ui::setup_tray(app.handle()) {
                Ok(t) => t,
                Err(e) => {
                    log::error!("Failed to setup system tray: {e}");
                    return Err(Box::new(e));
                }
            };

            // Link state machine transitions to tray icon updates
            let tray_handle = tray.clone();
            state_tracker_for_tray.subscribe(move |_old, new_state| {
                ui::update_tray_icon_for_state(&tray_handle, new_state);
            });

            // Start monitoring scheduler if enabled in configuration
            if controller_for_setup.config.read().enabled {
                let _ = controller_for_setup.state_tracker.transition(AppState::Monitoring);
                controller_for_setup.start_scheduler_loop();
            } else {
                let _ = controller_for_setup.state_tracker.transition(AppState::Disabled);
            }

            if launch_minimized {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppContainer>();
                let minimize_to_tray = state.controller.config.read().minimize_to_tray;

                if minimize_to_tray {
                    // Prevent window destroy and minimize to system tray instead
                    api.prevent_close();
                    let _ = window.hide();
                    log::info!("Window close intercepted: minimized to system tray");
                } else {
                    state.controller.shutdown();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app::get_telemetry,
            app::get_config,
            app::save_config,
            app::enable_monitoring,
            app::disable_monitoring,
            app::resume_monitoring,
            app::lock_windows_now,
            app::list_cameras,
            app::test_camera,
            app::test_detection,
            app::set_autostart,
            app::get_autostart,
            app::enroll_face,
            app::clear_enrolled_face,
            app::test_recognition,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running FaceGuard application");
}
