# Changelog

All notable changes to the FaceGuard project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-11-14

### Added
- **Local Face Identity Recognition**: Integrated edge neural network inference utilizing InsightFace MobileFaceNet (`models/face_recognition.onnx`, 13.6 MB) via pure-Rust `tract-onnx`.
- **512-Dimensional Biometric Embedding Extraction**: High-precision normalized facial vector extraction with cosine similarity calculation.
- **One-Click Face Enrollment**: Allows registering the owner's face directly from the webcam with live confidence verification.
- **Stranger / Intruder Auto-Lock Policy**: Configurable protection policy that detects unknown faces or strangers sitting in front of the screen and automatically locks the Windows workstation.
- **New Tauri IPC Endpoints**: Implemented `enroll_face`, `clear_enrolled_face`, and `test_recognition` commands.
- **Dashboard & Controls UI Expansion**: Added live identity recognition badge, real-time cosine similarity score display, enrollment action buttons, and threshold configuration.
- **Expanded Automated Test Suite**: Added 4 new integration tests covering owner recognition matching, stranger-induced auto-locks, face enrollment lifecycle, and ONNX model loading (13/13 tests passing).

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
- **Optimized Release Binary & Packaging**: Standalone 21.3 MB executable (`faceguard.exe`) compiled with Link-Time Optimization (LTO) and opt-level 3, packaged with model assets.
- **GitHub Repository Synchronization**: Version control initialized and published to [https://github.com/hamedhdd/faceguard](https://github.com/hamedhdd/faceguard).
