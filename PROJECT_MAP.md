# FaceGuard Codebase Map (`PROJECT_MAP.md`)

## 1. Directory Tree & Component Index

```text
faceguard/
├── Cargo.toml                       # Package definitions, pinned dependencies, release profiles
├── build.rs                         # Tauri 2 build integration script
├── tauri.conf.json                  # Application metadata, window geometry, bundle settings
├── README.md                        # Project documentation, setup guide, architecture overview
├── CHANGELOG.md                     # Semantic versioning changelog adhering to Keep a Changelog
├── PROJECT_MAP.md                   # Living architectural directory tree and telemetry map
├── models/
│   ├── face_detection.onnx          # Pretrained UltraFace RFB-320 ONNX neural network model
│   └── face_detection_yunet.onnx    # Pretrained YuNet ONNX neural network model
├── resources/
│   └── icons/                       # Pixel-perfect PNG and ICO assets for app & dynamic tray states
├── dist/                            # Self-contained, offline unslop frontend bundle
│   ├── index.html                   # Semantic HTML5 layout with SVG line icons
│   ├── style.css                    # Graphite ramp, 4px grid, nested radii, unslop styling
│   └── app.js                       # Frontend IPC client, telemetry poller, and DOM coordinator
├── src/
│   ├── lib.rs                       # Core library re-exports for application and integration tests
│   ├── main.rs                      # Application entry point, logging init, Tauri setup, window events
│   ├── app.rs                       # Tauri IPC command definitions and state container
│   ├── config.rs                    # JSON configuration management, validation, and safe defaults
│   ├── state.rs                     # Deterministic finite state machine and transition tracker
│   ├── scheduler.rs                 # Presence check interval and scheduling definitions
│   ├── camera/
│   │   ├── mod.rs                   # Camera module exports
│   │   ├── device.rs                # Camera error types and device descriptor structures
│   │   └── capture.rs               # MediaFoundation capture implementation & mock capture
│   ├── detection/
│   │   ├── mod.rs                   # Face detection module exports
│   │   ├── model.rs                 # Face bounding boxes, confidence, and summary structures
│   │   ├── detector.rs              # Tract-ONNX inference implementation & mock detector
│   │   └── recognition.rs           # MobileFaceNet 512-D embedding extraction & cosine similarity
│   ├── windows/
│   │   ├── mod.rs                   # Windows integration exports
│   │   ├── lock.rs                  # Native LockWorkStation() implementation & mock locker
│   │   ├── session.rs               # WTSRegisterSessionNotification Win32 listener & mock notifier
│   │   └── startup.rs               # HKCU\Run user startup registry manager
│   ├── monitoring/
│   │   ├── mod.rs                   # Monitoring module exports
│   │   └── controller.rs            # Orchestration engine, absence policy, and telemetry snapshot
│   └── ui/
│       ├── mod.rs                   # UI module exports
│       └── tray.rs                  # Dynamic system tray icon manager and context menu
└── tests/
    └── integration_tests.rs         # Automated test suite (13 test suites)
```

---

## 2. Data & Telemetry Flow

```mermaid
flowchart TD
    subgraph Capture["1. Frame Ingestion"]
        Webcam["DirectShow / MediaFoundation Webcam"] -->|Single RGB Frame| CamCapture["NokhwaCameraCapture"]
        CamCapture -->|Drop Device Handle| FreeHW["Hardware Released Immediately"]
    end

    subgraph Inference["2. Local Edge Inference"]
        CamCapture -->|RGB Image| Preprocess["Resize (320x240) + Normalize"]
        Preprocess -->|Tensor| TractONNX["tract-onnx CPU Inference Engine"]
        TractONNX -->|Tensors| Postprocess["Bounding Box & Confidence Decoder"]
        Postprocess -->|DetectionSummary| DropFrame["Drop Raw Frame & Memory"]
    end

    subgraph Evaluation["3. Policy Evaluation & State Machine"]
        Postprocess --> Controller["MonitoringController"]
        Controller -->|Face Found| ResetCounter["Reset failed_checks_counter to 0"]
        Controller -->|Face Missing| IncCounter["Increment failed_checks_counter"]
        IncCounter --> CheckThreshold{"failed_checks >= threshold?"}
        CheckThreshold -->|No| WaitNext["Wait for Next Interval (20s)"]
        CheckThreshold -->|Yes| LockReq["LockRequested"]
        LockReq --> LockWS["LockWorkStation() Native Windows Call"]
        LockWS --> Standby["Enter LockedStandby State"]
        Standby --> Freeze["Suspend Scheduler & Release Handles"]
    end

    subgraph Session["4. Zero-Polling Session Watcher"]
        Win32Worker["Dedicated Hidden Win32 Window"] -->|WTSRegisterSessionNotification| WTS["WM_WTSSESSION_CHANGE"]
        WTS -->|WTS_SESSION_UNLOCK| WakeUp["Resume from LockedStandby"]
        WakeUp --> VerifyCheck["Trigger Immediate Verification Scan"]
        VerifyCheck --> Controller
    end

    subgraph Presentation["5. UI & Tray Presentation"]
        Controller -->|TelemetrySnapshot (1s Interval)| TauriIPC["Tauri IPC Commands"]
        TauriIPC --> WebviewDOM["Frontend Dashboard (dist/app.js)"]
        Controller -->|State Change Event| TrayIcon["Tray Icon & Menu Update"]
    end
```

---

## 3. API & IPC Route Catalog

All IPC endpoints are invoked via `window.__TAURI__.core.invoke(command, args)`:

| Command | Arguments | Return Type | Description |
| :--- | :--- | :--- | :--- |
| `get_telemetry` | None | `TelemetrySnapshot` | Full operational metrics, states, timestamps, and counters |
| `get_config` | None | `AppConfig` | Current persistent configuration settings |
| `save_config` | `{ config: AppConfig }` | `Result<(), String>` | Validates, updates runtime config, and writes to disk |
| `enable_monitoring` | None | `Result<(), String>` | Resumes face monitoring and restarts scheduler loop |
| `disable_monitoring`| None | `Result<(), String>` | Kill switch: aborts checks and releases webcam hardware |
| `resume_monitoring` | None | `Result<(), String>` | Restarts monitoring from disabled state |
| `lock_windows_now`  | None | `Result<(), String>` | Immediately locks workstation and enters `LockedStandby` |
| `list_cameras`      | None | `Vec<CameraDeviceDescription>` | Enumerates available video capture devices |
| `test_camera`       | None | `CameraTestResult` | One-shot hardware capture test (resolution, format) |
| `test_detection`    | None | `DetectionTestResult` | One-shot face presence verification & inference latency |
| `enroll_face`       | None | `EnrollmentResult` | Captures frame, extracts 512-D embedding, and enrolls owner |
| `clear_enrolled_face`| None | `Result<(), String>` | Removes enrolled profile and reverts to general presence |
| `test_recognition`  | None | `RecognitionTestResult` | Compares live webcam frame against enrolled owner profile |
| `set_autostart`     | `{ enable: bool }` | `Result<(), String>` | Modifies `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| `get_autostart`     | None | `bool` | Checks if Windows startup registry key exists |

---

## 4. Configuration Schema

Stored at `%APPDATA%\FaceGuard\config.json`:

```json
{
  "enabled": true,
  "detection_interval_seconds": 20,
  "missing_face_threshold": 2,
  "confidence_threshold": 0.70,
  "minimum_face_size": 60,
  "camera_index": 0,
  "start_with_windows": false,
  "minimize_to_tray": true,
  "notifications_enabled": true,
  "lock_on_missing_face": true,
  "grace_period_seconds": 0,
  "recognition_enabled": false,
  "recognition_threshold": 0.60,
  "enrolled_face": null
}
```

---

## 5. Frontend / View Layout Catalog

- **Header Elements**:
  - `#statusPill`: Live application state indicator with colored status dot
  - `#statusText`: Text representation of state (Sentence case)
  - `#headerKillSwitch`: Emergency stop button
  - `#headerLockNow`: Instant manual workstation lock button
- **Tabs**:
  - `tab-btn[data-tab="dashboard"]` -> `#pane-dashboard`: Active telemetry and operational metrics
  - `tab-btn[data-tab="controls"]` -> `#pane-controls`: Manual overrides and hardware diagnostics
  - `tab-btn[data-tab="settings"]` -> `#pane-settings`: Configuration controls and registry options
- **Dashboard Metrics**:
  - `#metricAbsence`: Current consecutive failed checks / threshold
  - `#metricInterval`: Polling interval in seconds
  - `#metricLastDetect`: Timestamp of last confirmed face detection
  - `#metricLastConfidence`: Confidence score percentage of primary face
  - `#metricNextCheck`: Anticipated wall-clock time for subsequent scan
  - `#metricTotalChecks`: Lifetime check counter
  - `#deviceCameraName`: Human-readable device name of selected webcam
  - `#activityLog`: Reverse-chronological operational event log
- **Diagnostic Controls**:
  - `#btnTestCamera`: Triggers `test_camera`
  - `#btnTestDetection`: Triggers `test_detection`
  - `#diagOutput`: Monospace diagnostic log view
- **Settings Form**:
  - `#settingInterval`: Number input (5–300s)
  - `#settingThreshold`: Number input (1–10 checks)
  - `#settingConfidence`: Number input (0.50–0.95)
  - `#settingCameraSelect`: Dynamic select element populated from `list_cameras`
  - `#settingLockEnabled`: Toggle switch for `lock_on_missing_face`
  - `#settingStartWithWindows`: Toggle switch for Windows startup
  - `#settingMinimizeToTray`: Toggle switch for close-to-tray behavior
  - `#btnSaveSettings`: Persists configuration
