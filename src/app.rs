use crate::camera::{CameraCapture, CameraDeviceDescription};
use crate::config::AppConfig;
use crate::detection::FaceDetection;
use crate::monitoring::{MonitoringController, TelemetrySnapshot};
use crate::windows::{is_run_at_startup_enabled, set_run_at_startup};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

pub struct AppContainer {
    pub controller: Arc<MonitoringController>,
    pub camera_backend: Arc<dyn CameraCapture>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraTestResult {
    pub success: bool,
    pub width: u32,
    pub height: u32,
    pub camera_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionTestResult {
    pub success: bool,
    pub face_detected: bool,
    pub confidence_percent: f32,
    pub inference_ms: f32,
    pub primary_face: Option<FaceDetection>,
    pub message: String,
}

#[tauri::command]
pub fn get_telemetry(state: State<'_, AppContainer>) -> TelemetrySnapshot {
    state.controller.get_telemetry()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppContainer>) -> AppConfig {
    state.controller.config.read().clone()
}

#[tauri::command]
pub fn save_config(config: AppConfig, state: State<'_, AppContainer>) -> Result<(), String> {
    // If startup setting changed, apply to Windows registry
    let old_startup = state.controller.config.read().start_with_windows;
    if old_startup != config.start_with_windows {
        let _ = set_run_at_startup(config.start_with_windows);
    }

    state.controller.update_config(config)
}

#[tauri::command]
pub fn enable_monitoring(state: State<'_, AppContainer>) -> Result<(), String> {
    state.controller.resume_monitoring();
    Ok(())
}

#[tauri::command]
pub fn disable_monitoring(state: State<'_, AppContainer>) -> Result<(), String> {
    state.controller.disable_monitoring();
    Ok(())
}

#[tauri::command]
pub fn resume_monitoring(state: State<'_, AppContainer>) -> Result<(), String> {
    state.controller.resume_monitoring();
    Ok(())
}

#[tauri::command]
pub fn lock_windows_now(state: State<'_, AppContainer>) -> Result<(), String> {
    state.controller.lock_now()
}

#[tauri::command]
pub fn list_cameras(state: State<'_, AppContainer>) -> Result<Vec<CameraDeviceDescription>, String> {
    state.camera_backend.list_cameras().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn test_camera(state: State<'_, AppContainer>) -> Result<CameraTestResult, String> {
    let cam_idx = state.controller.config.read().camera_index;
    let cam_name = state.controller.get_telemetry().camera_name;

    match state.camera_backend.capture_single_frame(cam_idx) {
        Ok(frame) => {
            let w = frame.width();
            let h = frame.height();
            drop(frame); // Frame released immediately
            Ok(CameraTestResult {
                success: true,
                width: w,
                height: h,
                camera_name: cam_name,
                message: format!("Successfully captured 1 frame ({w}x{h}). Camera device released."),
            })
        }
        Err(e) => Ok(CameraTestResult {
            success: false,
            width: 0,
            height: 0,
            camera_name: cam_name,
            message: format!("Camera test failed: {e}"),
        }),
    }
}

#[tauri::command]
pub fn test_detection(state: State<'_, AppContainer>) -> Result<DetectionTestResult, String> {
    match state.controller.perform_check() {
        Ok(summary) => Ok(DetectionTestResult {
            success: true,
            face_detected: summary.is_face_present,
            confidence_percent: summary.max_confidence * 100.0,
            inference_ms: summary.inference_duration_ms,
            primary_face: summary.primary_face,
            message: if summary.is_face_present {
                format!(
                    "Face detected! Confidence: {:.1}%, inference time: {:.1}ms",
                    summary.max_confidence * 100.0,
                    summary.inference_duration_ms
                )
            } else {
                format!(
                    "No face detected in frame. Max confidence: {:.1}%, inference time: {:.1}ms",
                    summary.max_confidence * 100.0,
                    summary.inference_duration_ms
                )
            },
        }),
        Err(e) => Ok(DetectionTestResult {
            success: false,
            face_detected: false,
            confidence_percent: 0.0,
            inference_ms: 0.0,
            primary_face: None,
            message: format!("Detection test failed: {e}"),
        }),
    }
}

#[tauri::command]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    set_run_at_startup(enable)
}

#[tauri::command]
pub fn get_autostart() -> bool {
    is_run_at_startup_enabled()
}
