//! Scheduler utilities for periodic face-presence polling.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulePolicy {
    pub interval: Duration,
    pub missing_face_threshold: u32,
}

impl Default for SchedulePolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(20),
            missing_face_threshold: 2,
        }
    }
}
