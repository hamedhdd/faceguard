use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;

/// Application state machine states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    Initializing,
    Disabled,
    Monitoring,
    FaceDetected,
    FaceMissing,
    LockRequested,
    LockedStandby,
    CameraError,
    ShuttingDown,
}

impl fmt::Display for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            AppState::Initializing => "Initializing",
            AppState::Disabled => "Disabled",
            AppState::Monitoring => "Monitoring",
            AppState::FaceDetected => "Face Detected",
            AppState::FaceMissing => "Face Missing",
            AppState::LockRequested => "Lock Requested",
            AppState::LockedStandby => "Locked Standby",
            AppState::CameraError => "Camera Error",
            AppState::ShuttingDown => "Shutting Down",
        };
        write!(f, "{name}")
    }
}

/// Verifies whether a transition from `from` to `to` is legally allowed.
pub fn is_valid_transition(from: AppState, to: AppState) -> bool {
    if from == to {
        return true;
    }
    // Any state can transition to ShuttingDown
    if to == AppState::ShuttingDown {
        return true;
    }

    match from {
        AppState::Initializing => matches!(to, AppState::Monitoring | AppState::Disabled | AppState::CameraError),
        AppState::Monitoring => matches!(
            to,
            AppState::FaceDetected | AppState::FaceMissing | AppState::CameraError | AppState::Disabled | AppState::LockRequested | AppState::LockedStandby
        ),
        AppState::FaceDetected => matches!(
            to,
            AppState::FaceMissing | AppState::Monitoring | AppState::CameraError | AppState::Disabled | AppState::LockRequested
        ),
        AppState::FaceMissing => matches!(
            to,
            AppState::FaceDetected | AppState::LockRequested | AppState::LockedStandby | AppState::CameraError | AppState::Disabled | AppState::Monitoring
        ),
        AppState::LockRequested => matches!(to, AppState::LockedStandby | AppState::Disabled),
        AppState::LockedStandby => matches!(to, AppState::Monitoring | AppState::Disabled | AppState::Initializing),
        AppState::CameraError => matches!(to, AppState::Monitoring | AppState::Disabled | AppState::FaceDetected | AppState::FaceMissing),
        AppState::Disabled => matches!(to, AppState::Monitoring | AppState::Initializing),
        AppState::ShuttingDown => false,
    }
}

pub type StateChangeCallback = Arc<dyn Fn(AppState, AppState) + Send + Sync + 'static>;

/// Thread-safe state tracker and dispatcher.
pub struct StateTracker {
    current: RwLock<AppState>,
    transition_count: AtomicUsize,
    listeners: RwLock<Vec<StateChangeCallback>>,
}

impl StateTracker {
    pub fn new(initial: AppState) -> Self {
        Self {
            current: RwLock::new(initial),
            transition_count: AtomicUsize::new(0),
            listeners: RwLock::new(Vec::new()),
        }
    }

    pub fn current(&self) -> AppState {
        *self.current.read()
    }

    pub fn transition_count(&self) -> usize {
        self.transition_count.load(Ordering::Relaxed)
    }

    pub fn subscribe<F>(&self, cb: F)
    where
        F: Fn(AppState, AppState) + Send + Sync + 'static,
    {
        self.listeners.write().push(Arc::new(cb));
    }

    /// Attempts to transition to `new_state`. Returns Ok(true) if state changed,
    /// Ok(false) if already in `new_state`, or Err if transition is invalid.
    pub fn transition(&self, new_state: AppState) -> Result<bool, String> {
        let (old_state, callbacks) = {
            let mut lock = self.current.write();
            let old = *lock;
            if old == new_state {
                return Ok(false);
            }

            if !is_valid_transition(old, new_state) {
                return Err(format!("Invalid state transition from '{old}' to '{new_state}'"));
            }

            *lock = new_state;
            self.transition_count.fetch_add(1, Ordering::Relaxed);
            let cbs = self.listeners.read().clone();
            (old, cbs)
        };

        log::info!("State machine transition: {old_state} -> {new_state}");

        // Dispatch notifications outside the lock
        for cb in callbacks {
            cb(old_state, new_state);
        }

        Ok(true)
    }

    /// Force set state (e.g. for emergency stop or testing)
    pub fn force_set(&self, new_state: AppState) {
        let (old, callbacks) = {
            let mut lock = self.current.write();
            let old = *lock;
            *lock = new_state;
            self.transition_count.fetch_add(1, Ordering::Relaxed);
            let cbs = self.listeners.read().clone();
            (old, cbs)
        };

        log::warn!("State machine force transition: {old} -> {new_state}");
        for cb in callbacks {
            cb(old, new_state);
        }
    }
}
