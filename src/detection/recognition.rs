use super::model::{FaceDetection, RecognitionError};
use image::{imageops::FilterType, RgbImage};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tract_onnx::prelude::*;

pub type RunnableModel = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

/// Abstraction contract for face identity recognition and embedding extraction.
pub trait FaceRecognizer: Send + Sync {
    /// Extracts a normalized embedding vector (512-D) from the cropped face region in the frame.
    fn extract_embedding(&self, frame: &RgbImage, face: &FaceDetection) -> Result<Vec<f32>, RecognitionError>;

    /// Computes the cosine similarity between two normalized face embeddings. Returns 0.0 to 1.0.
    fn compute_similarity(&self, emb1: &[f32], emb2: &[f32]) -> f32;
}

/// Pure-Rust ONNX face recognition engine utilizing MobileFaceNet / InsightFace.
pub struct OnnxFaceRecognizer {
    model: RunnableModel,
}

impl OnnxFaceRecognizer {
    /// Creates a new face recognizer from a model file path, or falls back to standard search paths.
    pub fn new<P: AsRef<Path>>(preferred_path: Option<P>) -> Result<Self, RecognitionError> {
        let candidates = [
            preferred_path.map(|p| p.as_ref().to_path_buf()),
            Some(PathBuf::from("models/face_recognition.onnx")),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("models").join("face_recognition.onnx"))),
        ];

        let mut chosen_path = None;
        for candidate in candidates.into_iter().flatten() {
            if candidate.exists() {
                chosen_path = Some(candidate);
                break;
            }
        }

        let model_path = chosen_path.ok_or_else(|| RecognitionError::ModelLoadError {
            path: "models/face_recognition.onnx".into(),
            detail: "Model file not found in search paths".into(),
        })?;

        log::info!("Loading face recognition ONNX model from: {:?}", model_path);

        let model = tract_onnx::onnx()
            .model_for_path(&model_path)
            .map_err(|e| RecognitionError::ModelLoadError {
                path: model_path.to_string_lossy().to_string(),
                detail: e.to_string(),
            })?
            .with_input_fact(0, f32::fact([1, 3, 112, 112]).into())
            .map_err(|e| RecognitionError::ModelLoadError {
                path: model_path.to_string_lossy().to_string(),
                detail: format!("Failed to set input fact: {e}"),
            })?
            .into_optimized()
            .map_err(|e| RecognitionError::ModelLoadError {
                path: model_path.to_string_lossy().to_string(),
                detail: format!("Optimization failed: {e}"),
            })?
            .into_runnable()
            .map_err(|e| RecognitionError::ModelLoadError {
                path: model_path.to_string_lossy().to_string(),
                detail: format!("Failed to create runnable model: {e}"),
            })?;

        Ok(Self { model })
    }
}

impl FaceRecognizer for OnnxFaceRecognizer {
    fn extract_embedding(&self, frame: &RgbImage, face: &FaceDetection) -> Result<Vec<f32>, RecognitionError> {
        let [xmin, ymin, xmax, ymax] = face.box_pixel;
        let width = frame.width();
        let height = frame.height();

        // 12% padding around the face bounding box for head silhouette context
        let box_w = xmax.saturating_sub(xmin);
        let box_h = ymax.saturating_sub(ymin);
        if box_w < 10 || box_h < 10 {
            return Err(RecognitionError::CropError("Face bounding box is too small".into()));
        }

        let pad_w = (box_w as f32 * 0.12) as u32;
        let pad_h = (box_h as f32 * 0.12) as u32;
        let crop_x = xmin.saturating_sub(pad_w);
        let crop_y = ymin.saturating_sub(pad_h);
        let crop_w = (xmax + pad_w).min(width).saturating_sub(crop_x);
        let crop_h = (ymax + pad_h).min(height).saturating_sub(crop_y);

        let cropped = image::imageops::crop_imm(frame, crop_x, crop_y, crop_w, crop_h).to_image();
        let resized = image::imageops::resize(&cropped, 112, 112, FilterType::Triangle);

        // Convert RGB into planar tensor format: [1, 3, 112, 112] normalized to [-1.0, 1.0]
        let mut data = Vec::with_capacity(3 * 112 * 112);
        for c in 0..3 {
            for y in 0..112 {
                for x in 0..112 {
                    let pixel = resized.get_pixel(x, y);
                    let channel_val = pixel[c] as f32;
                    data.push((channel_val - 127.5) / 127.5);
                }
            }
        }

        let tensor = tract_onnx::prelude::Tensor::from_shape(&[1, 3, 112, 112], &data)
            .map_err(|e| RecognitionError::InferenceError(e.to_string()))?;

        let outputs = self
            .model
            .run(tvec!(tensor.into()))
            .map_err(|e| RecognitionError::InferenceError(e.to_string()))?;

        if outputs.is_empty() {
            return Err(RecognitionError::InferenceError("Empty output from recognition model".into()));
        }

        let out_view = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| RecognitionError::DimensionError(e.to_string()))?;

        let slice = out_view
            .as_slice()
            .ok_or_else(|| RecognitionError::DimensionError("Non-contiguous model output".into()))?;

        // L2 normalize the embedding vector
        let norm = (slice.iter().map(|&v| v * v).sum::<f32>()).sqrt().max(1e-6);
        let normalized = slice.iter().map(|&v| v / norm).collect::<Vec<f32>>();

        Ok(normalized)
    }

    fn compute_similarity(&self, emb1: &[f32], emb2: &[f32]) -> f32 {
        if emb1.len() != emb2.len() || emb1.is_empty() {
            return 0.0;
        }
        let dot: f32 = emb1.iter().zip(emb2.iter()).map(|(a, b)| a * b).sum();
        dot.max(0.0).min(1.0)
    }
}

/// Mock face recognizer for deterministic automated testing.
pub struct MockFaceRecognizer {
    pub similarity: Mutex<f32>,
    pub mock_embedding: Vec<f32>,
    pub should_fail: Mutex<bool>,
}

impl MockFaceRecognizer {
    pub fn new(similarity: f32) -> Self {
        Self {
            similarity: Mutex::new(similarity),
            mock_embedding: vec![0.1; 512],
            should_fail: Mutex::new(false),
        }
    }

    pub fn set_similarity(&self, val: f32) {
        if let Ok(mut lock) = self.similarity.lock() {
            *lock = val;
        }
    }

    pub fn set_should_fail(&self, fail: bool) {
        if let Ok(mut lock) = self.should_fail.lock() {
            *lock = fail;
        }
    }
}

impl FaceRecognizer for MockFaceRecognizer {
    fn extract_embedding(&self, _frame: &RgbImage, _face: &FaceDetection) -> Result<Vec<f32>, RecognitionError> {
        if let Ok(fail) = self.should_fail.lock() {
            if *fail {
                return Err(RecognitionError::InferenceError("Mock inference error".into()));
            }
        }
        Ok(self.mock_embedding.clone())
    }

    fn compute_similarity(&self, _emb1: &[f32], _emb2: &[f32]) -> f32 {
        *self.similarity.lock().unwrap_or_else(|e| e.into_inner())
    }
}
