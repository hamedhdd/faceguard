use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("Failed to load model file at {path}: {detail}")]
    ModelLoadError { path: String, detail: String },
    #[error("Failed to build input tensor: {0}")]
    TensorError(String),
    #[error("Inference execution failed: {0}")]
    InferenceError(String),
    #[error("Model output decoding failed: {0}")]
    DecodeError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaceDetection {
    /// Confidence score from 0.0 to 1.0
    pub confidence: f32,
    /// Normalized coordinates: [xmin, ymin, xmax, ymax] in range 0.0..=1.0
    pub box_normalized: [f32; 4],
    /// Absolute pixel coordinates: [xmin, ymin, xmax, ymax] in original frame
    pub box_pixel: [u32; 4],
    /// Face width in pixels
    pub width: u32,
    /// Face height in pixels
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetectionSummary {
    /// True if at least one face meets confidence and size thresholds
    pub is_face_present: bool,
    /// Total number of faces meeting criteria
    pub faces_count: usize,
    /// Highest confidence score observed in the frame
    pub max_confidence: f32,
    /// Primary face (highest confidence)
    pub primary_face: Option<FaceDetection>,
    /// Time spent running the model inference in milliseconds
    pub inference_duration_ms: f32,
    /// Timestamp of detection
    pub timestamp: chrono::DateTime<chrono::Local>,
}
