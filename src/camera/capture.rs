use super::device::{CameraDeviceDescription, CameraError};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;
use parking_lot::Mutex;

/// Abstract camera capture trait (DIP / ISP).
pub trait CameraCapture: Send + Sync {
    /// Captures a single 2D RGB frame and immediately releases all camera handles.
    fn capture_single_frame(&self, camera_index: u32) -> Result<image::RgbImage, CameraError>;

    /// Lists available system webcams.
    fn list_cameras(&self) -> Result<Vec<CameraDeviceDescription>, CameraError>;
}

/// Production camera capture implementation using native Media Foundation via Nokhwa.
pub struct NokhwaCameraCapture;

impl NokhwaCameraCapture {
    pub fn new() -> Self {
        Self
    }
}

impl CameraCapture for NokhwaCameraCapture {
    fn list_cameras(&self) -> Result<Vec<CameraDeviceDescription>, CameraError> {
        let cameras = nokhwa::query(ApiBackend::MediaFoundation)
            .map_err(|e| CameraError::InitializationFailed(e.to_string()))?;

        let mut list = Vec::new();
        for (i, cam) in cameras.into_iter().enumerate() {
            list.push(CameraDeviceDescription {
                index: i as u32,
                name: cam.human_name(),
                is_default: i == 0,
            });
        }
        Ok(list)
    }

    fn capture_single_frame(&self, camera_index: u32) -> Result<image::RgbImage, CameraError> {
        let index = CameraIndex::Index(camera_index);
        let requested = RequestedFormat::new::<RgbFormat>(
            RequestedFormatType::AbsoluteHighestFrameRate,
        );

        let mut camera = Camera::new(index, requested)
            .map_err(|_e| CameraError::NotFound(camera_index))?;

        camera.open_stream()
            .map_err(|e| CameraError::StreamFailed(e.to_string()))?;

        // Guard to ensure stream is stopped and camera released even if decode fails
        let frame_res = camera.frame();
        let _ = camera.stop_stream();

        let frame = frame_res.map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        let decoded = frame.decode_image::<RgbFormat>()
            .map_err(|e| CameraError::DecodeFailed(e.to_string()))?;

        Ok(decoded)
    }
}

/// Mock camera capture implementation for automated unit & integration testing.
pub struct MockCameraCapture {
    pub cameras: Vec<CameraDeviceDescription>,
    pub frame_to_return: Mutex<Option<image::RgbImage>>,
    pub fail_with: Mutex<Option<String>>,
}

impl MockCameraCapture {
    pub fn new() -> Self {
        Self {
            cameras: vec![CameraDeviceDescription {
                index: 0,
                name: "Mock Virtual Camera".to_string(),
                is_default: true,
            }],
            frame_to_return: Mutex::new(Some(image::RgbImage::new(640, 480))),
            fail_with: Mutex::new(None),
        }
    }

    pub fn set_frame(&self, img: image::RgbImage) {
        *self.frame_to_return.lock() = Some(img);
    }

    pub fn set_failure(&self, err_msg: Option<String>) {
        *self.fail_with.lock() = err_msg;
    }
}

impl CameraCapture for MockCameraCapture {
    fn list_cameras(&self) -> Result<Vec<CameraDeviceDescription>, CameraError> {
        if let Some(err) = self.fail_with.lock().as_ref() {
            return Err(CameraError::InitializationFailed(err.clone()));
        }
        Ok(self.cameras.clone())
    }

    fn capture_single_frame(&self, camera_index: u32) -> Result<image::RgbImage, CameraError> {
        if let Some(err) = self.fail_with.lock().as_ref() {
            return Err(CameraError::CaptureFailed(err.clone()));
        }

        if camera_index as usize >= self.cameras.len() {
            return Err(CameraError::NotFound(camera_index));
        }

        match self.frame_to_return.lock().clone() {
            Some(frame) => Ok(frame),
            None => Err(CameraError::CaptureFailed("No mock frame configured".to_string())),
        }
    }
}
