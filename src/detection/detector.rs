use super::model::{DetectionError, DetectionSummary, FaceDetection};
use parking_lot::Mutex;
use std::path::Path;
use std::time::Instant;
use tract_onnx::prelude::*;

pub trait FaceDetector: Send + Sync {
    /// Detects human faces in an RGB image.
    fn detect(
        &self,
        frame: &image::RgbImage,
        confidence_threshold: f32,
        min_size_px: u32,
    ) -> Result<DetectionSummary, DetectionError>;
}

/// Production face detector using tract-onnx.
pub struct OnnxFaceDetector {
    model: SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    input_width: u32,
    input_height: u32,
}

impl OnnxFaceDetector {
    /// Loads an ONNX face detection model from the specified file path.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, DetectionError> {
        let p = path.as_ref();
        let path_str = p.display().to_string();

        let model = tract_onnx::onnx()
            .model_for_path(p)
            .map_err(|e| DetectionError::ModelLoadError {
                path: path_str.clone(),
                detail: format!("Failed to parse ONNX model: {e:?}"),
            })?
            .with_input_fact(0, f32::fact([1, 3, 240, 320]).into())
            .map_err(|e| DetectionError::ModelLoadError {
                path: path_str.clone(),
                detail: format!("Failed to set input fact: {e:?}"),
            })?
            .into_optimized()
            .map_err(|e| DetectionError::ModelLoadError {
                path: path_str.clone(),
                detail: format!("Failed to optimize model graph: {e:?}"),
            })?
            .into_runnable()
            .map_err(|e| DetectionError::ModelLoadError {
                path: path_str.clone(),
                detail: format!("Failed to compile runnable model: {e:?}"),
            })?;

        log::info!("ONNX face detection model loaded successfully from {:?}", p);

        Ok(Self {
            model,
            input_width: 320,
            input_height: 240,
        })
    }
}

impl FaceDetector for OnnxFaceDetector {
    fn detect(
        &self,
        frame: &image::RgbImage,
        confidence_threshold: f32,
        min_size_px: u32,
    ) -> Result<DetectionSummary, DetectionError> {
        let orig_w = frame.width();
        let orig_h = frame.height();

        // 1. Resize to model input dimensions (320x240)
        let resized = image::imageops::resize(
            frame,
            self.input_width,
            self.input_height,
            image::imageops::FilterType::Triangle,
        );

        // 2. Prepare normalized tensor [1, 3, 240, 320]
        let mut tensor = tract_onnx::prelude::Tensor::zero::<f32>(&[
            1,
            3,
            self.input_height as usize,
            self.input_width as usize,
        ])
        .map_err(|e| DetectionError::TensorError(e.to_string()))?;

        {
            let mut view = tensor
                .to_array_view_mut::<f32>()
                .map_err(|e| DetectionError::TensorError(e.to_string()))?;

            for y in 0..self.input_height {
                for x in 0..self.input_width {
                    let px = resized.get_pixel(x, y);
                    // Standard RFB / UltraFace normalization: (x - 127.0) / 128.0
                    view[[0, 0, y as usize, x as usize]] = (px[0] as f32 - 127.0) / 128.0;
                    view[[0, 1, y as usize, x as usize]] = (px[1] as f32 - 127.0) / 128.0;
                    view[[0, 2, y as usize, x as usize]] = (px[2] as f32 - 127.0) / 128.0;
                }
            }
        }

        // 3. Run inference
        let start = Instant::now();
        let outputs = self
            .model
            .run(tvec!(tensor.into()))
            .map_err(|e| DetectionError::InferenceError(e.to_string()))?;
        let duration = start.elapsed();
        let duration_ms = duration.as_secs_f32() * 1000.0;

        if outputs.len() < 2 {
            return Err(DetectionError::DecodeError(
                "Model produced fewer than 2 output tensors".to_string(),
            ));
        }

        let scores = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| DetectionError::DecodeError(e.to_string()))?;
        let boxes = outputs[1]
            .to_array_view::<f32>()
            .map_err(|e| DetectionError::DecodeError(e.to_string()))?;

        let num_priors = scores.shape()[1];
        let mut max_conf = 0.0f32;
        let mut detections = Vec::new();

        for i in 0..num_priors {
            let conf = scores[[0, i, 1]]; // Class 1 is face
            if conf > max_conf {
                max_conf = conf;
            }

            if conf >= confidence_threshold {
                let xmin_norm = boxes[[0, i, 0]].clamp(0.0, 1.0);
                let ymin_norm = boxes[[0, i, 1]].clamp(0.0, 1.0);
                let xmax_norm = boxes[[0, i, 2]].clamp(0.0, 1.0);
                let ymax_norm = boxes[[0, i, 3]].clamp(0.0, 1.0);

                let xmin_px = (xmin_norm * orig_w as f32).round() as u32;
                let ymin_px = (ymin_norm * orig_h as f32).round() as u32;
                let xmax_px = (xmax_norm * orig_w as f32).round() as u32;
                let ymax_px = (ymax_norm * orig_h as f32).round() as u32;

                let face_w = xmax_px.saturating_sub(xmin_px);
                let face_h = ymax_px.saturating_sub(ymin_px);

                // Filter by minimum face size
                if face_w >= min_size_px && face_h >= min_size_px {
                    detections.push(FaceDetection {
                        confidence: conf,
                        box_normalized: [xmin_norm, ymin_norm, xmax_norm, ymax_norm],
                        box_pixel: [xmin_px, ymin_px, xmax_px, ymax_px],
                        width: face_w,
                        height: face_h,
                    });
                }
            }
        }

        // Sort detections by descending confidence
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

        let is_present = !detections.is_empty();
        let primary_face = detections.first().cloned();

        Ok(DetectionSummary {
            is_face_present: is_present,
            faces_count: detections.len(),
            max_confidence: max_conf,
            primary_face,
            inference_duration_ms: duration_ms,
            timestamp: chrono::Local::now(),
            recognition: None,
        })
    }
}

/// Mock face detector for deterministic automated tests.
pub struct MockFaceDetector {
    pub face_present: Mutex<bool>,
    pub confidence: Mutex<f32>,
    pub should_fail: Mutex<bool>,
}

impl MockFaceDetector {
    pub fn new(face_present: bool, confidence: f32) -> Self {
        Self {
            face_present: Mutex::new(face_present),
            confidence: Mutex::new(confidence),
            should_fail: Mutex::new(false),
        }
    }

    pub fn set_face_present(&self, present: bool) {
        *self.face_present.lock() = present;
    }

    pub fn set_confidence(&self, conf: f32) {
        *self.confidence.lock() = conf;
    }

    pub fn set_should_fail(&self, fail: bool) {
        *self.should_fail.lock() = fail;
    }
}

impl FaceDetector for MockFaceDetector {
    fn detect(
        &self,
        _frame: &image::RgbImage,
        confidence_threshold: f32,
        _min_size_px: u32,
    ) -> Result<DetectionSummary, DetectionError> {
        if *self.should_fail.lock() {
            return Err(DetectionError::InferenceError("Mock inference failure".to_string()));
        }

        let present = *self.face_present.lock();
        let conf = *self.confidence.lock();

        let is_detected = present && conf >= confidence_threshold;

        let primary_face = if is_detected {
            Some(FaceDetection {
                confidence: conf,
                box_normalized: [0.3, 0.2, 0.7, 0.8],
                box_pixel: [192, 96, 448, 384],
                width: 256,
                height: 288,
            })
        } else {
            None
        };

        Ok(DetectionSummary {
            is_face_present: is_detected,
            faces_count: if is_detected { 1 } else { 0 },
            max_confidence: conf,
            primary_face,
            inference_duration_ms: 12.5,
            timestamp: chrono::Local::now(),
            recognition: None,
        })
    }
}
