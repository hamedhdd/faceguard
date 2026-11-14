use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CameraError {
    #[error("No camera found or index {0} out of bounds")]
    NotFound(u32),
    #[error("Camera is busy or occupied by another application")]
    Busy(String),
    #[error("Failed to initialize camera backend: {0}")]
    InitializationFailed(String),
    #[error("Failed to open camera stream: {0}")]
    StreamFailed(String),
    #[error("Failed to capture frame: {0}")]
    CaptureFailed(String),
    #[error("Failed to decode frame format: {0}")]
    DecodeFailed(String),
    #[error("Operation cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CameraDeviceDescription {
    pub index: u32,
    pub name: String,
    pub is_default: bool,
}
