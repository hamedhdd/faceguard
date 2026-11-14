# Changelog

All notable changes to the FaceGuard project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-11-14

### Added
- **Core Monitoring Architecture**: Periodic single-frame capture loop with automatic release of camera hardware handles between checks.
- **Hardware-Agnostic Computer Vision**: Local CPU inference engine utilizing `tract-onnx` and pretrained UltraFace RFB-320 & YuNet neural networks for frontal and rotated face presence detection.
- **Windows Session Notification Handler**: Dedicated Win32 worker thread registering `WTSRegisterSessionNotification` for `WTS_SESSION_LOCK` and `WTS_SESSION_UNLOCK` events.
- **Native Locking Integration**: Workstation locking via `windows::Win32::System::Shutdown::LockWorkStation()`.
- **False-Positive Prevention System**: Configurable consecutive failed-check absence policy (default: 2 checks / 40s) that resets immediately when a face returns.
- **Hardware Error Isolation**: Robust error handling that isolates camera disconnections and occupancy errors without triggering accidental workstation locks.
- **Emergency Kill Switch**: Immediate cessation of scheduled checks and camera handle release via main UI and system tray.
- **Modern Desktop UI**: Tauri 2 desktop application styled according to `unslop` and `impeccable` design specifications (Operate mode, Graphite ramp, 4px grid, tabular numbers, clean SVG icons).
- **Dynamic System Tray**: System tray icon with visual state differentiation (Monitoring, Disabled, Locked Standby, Camera Error) and context menu controls.
- **Windows Auto-Start Manager**: Clean registry management under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` requiring zero elevated permissions.
- **Automated Test Suite**: 9 comprehensive test modules covering state machine transitions, threshold logic, error isolation, kill switch, scheduler cancellation, lock requests, locked standby, unlock resume, and config validation.
