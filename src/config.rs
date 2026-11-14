use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn default_recognition_threshold() -> f32 {
    0.60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// Whether face monitoring is active
    pub enabled: bool,
    /// Detection interval in seconds (default: 20)
    pub detection_interval_seconds: u64,
    /// Number of consecutive failed checks before locking (default: 2)
    pub missing_face_threshold: u32,
    /// Minimum face detection confidence threshold (0.0 - 1.0, default: 0.70)
    pub confidence_threshold: f32,
    /// Minimum face bounding box size in pixels (default: 60)
    pub minimum_face_size: u32,
    /// Selected camera index (default: 0)
    pub camera_index: u32,
    /// Automatically start with Windows
    pub start_with_windows: bool,
    /// Minimize to system tray on window close
    pub minimize_to_tray: bool,
    /// Show desktop notifications on events
    pub notifications_enabled: bool,
    /// Perform actual Windows workstation lock when threshold is reached
    pub lock_on_missing_face: bool,
    /// Optional grace period in seconds before locking (default: 0)
    pub grace_period_seconds: u64,
    /// Whether owner face recognition matching is enforced (default: false)
    #[serde(default)]
    pub recognition_enabled: bool,
    /// Minimum cosine similarity required to match enrolled face (0.10 - 0.99, default: 0.60)
    #[serde(default = "default_recognition_threshold")]
    pub recognition_threshold: f32,
    /// 512-dimensional enrolled owner face embedding vector
    #[serde(default)]
    pub enrolled_face: Option<Vec<f32>>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            detection_interval_seconds: 20,
            missing_face_threshold: 2,
            confidence_threshold: 0.70,
            minimum_face_size: 60,
            camera_index: 0,
            start_with_windows: false,
            minimize_to_tray: true,
            notifications_enabled: true,
            lock_on_missing_face: true,
            grace_period_seconds: 0,
            recognition_enabled: false,
            recognition_threshold: 0.60,
            enrolled_face: None,
        }
    }
}

impl AppConfig {
    /// Validates configuration values and clamps out-of-range settings to safe limits.
    pub fn validate(&mut self) {
        // Clamp detection interval between 3 and 3600 seconds
        if self.detection_interval_seconds < 3 {
            log::warn!("detection_interval_seconds was {}, clamped to min 3", self.detection_interval_seconds);
            self.detection_interval_seconds = 3;
        } else if self.detection_interval_seconds > 3600 {
            log::warn!("detection_interval_seconds was {}, clamped to max 3600", self.detection_interval_seconds);
            self.detection_interval_seconds = 3600;
        }

        // Clamp missing face threshold between 1 and 20 checks
        if self.missing_face_threshold < 1 {
            log::warn!("missing_face_threshold was 0, clamped to 1");
            self.missing_face_threshold = 1;
        } else if self.missing_face_threshold > 20 {
            log::warn!("missing_face_threshold was {}, clamped to 20", self.missing_face_threshold);
            self.missing_face_threshold = 20;
        }

        // Confidence threshold between 0.10 and 0.99
        if self.confidence_threshold < 0.10 {
            self.confidence_threshold = 0.10;
        } else if self.confidence_threshold > 0.99 {
            self.confidence_threshold = 0.99;
        }

        // Minimum face size between 16 and 400 pixels
        if self.minimum_face_size < 16 {
            self.minimum_face_size = 16;
        } else if self.minimum_face_size > 400 {
            self.minimum_face_size = 400;
        }

        // Recognition threshold between 0.10 and 0.99
        if self.recognition_threshold < 0.10 {
            self.recognition_threshold = 0.10;
        } else if self.recognition_threshold > 0.99 {
            self.recognition_threshold = 0.99;
        }

        if self.grace_period_seconds > 300 {
            self.grace_period_seconds = 300;
        }
    }

    /// Resolves the default config file path in %APPDATA%\FaceGuard\config.json
    pub fn default_config_path() -> PathBuf {
        if let Ok(app_data) = std::env::var("APPDATA") {
            let dir = Path::new(&app_data).join("FaceGuard");
            let _ = fs::create_dir_all(&dir);
            dir.join("config.json")
        } else {
            PathBuf::from("config.json")
        }
    }

    /// Loads configuration from specified path, falling back to defaults if missing or corrupted.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Self {
        let p = path.as_ref();
        if !p.exists() {
            log::info!("Config file not found at {:?}, using defaults", p);
            let mut cfg = Self::default();
            cfg.validate();
            let _ = cfg.save_to_path(p);
            return cfg;
        }

        match fs::read_to_string(p) {
            Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                Ok(mut cfg) => {
                    cfg.validate();
                    log::info!("Config loaded successfully from {:?}", p);
                    cfg
                }
                Err(e) => {
                    log::error!("Config file corrupt at {:?} ({e}), using defaults", p);
                    let mut cfg = Self::default();
                    cfg.validate();
                    cfg
                }
            },
            Err(e) => {
                log::error!("Failed to read config file at {:?} ({e}), using defaults", p);
                let mut cfg = Self::default();
                cfg.validate();
                cfg
            }
        }
    }

    /// Saves configuration to the specified path.
    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> Result<(), std::io::Error> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(p, json)
    }

    pub fn load() -> Self {
        Self::load_from_path(Self::default_config_path())
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        self.save_to_path(Self::default_config_path())
    }
}
