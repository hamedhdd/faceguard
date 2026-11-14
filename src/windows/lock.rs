use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("Windows LockWorkStation API failed: {0}")]
    ApiFailed(String),
}

pub trait WorkstationLocker: Send + Sync {
    /// Locks the Windows interactive workstation session.
    fn lock_workstation(&self) -> Result<(), LockError>;
}

/// Production Windows locker using official Windows API.
pub struct NativeWindowsLocker;

impl NativeWindowsLocker {
    pub fn new() -> Self {
        Self
    }
}

impl WorkstationLocker for NativeWindowsLocker {
    fn lock_workstation(&self) -> Result<(), LockError> {
        #[cfg(windows)]
        {
            log::info!("Calling Windows native LockWorkStation()...");
            unsafe {
                windows::Win32::System::Shutdown::LockWorkStation()
                    .map_err(|e| LockError::ApiFailed(e.to_string()))?;
            }
            log::info!("Windows workstation successfully locked.");
            Ok(())
        }
        #[cfg(not(windows))]
        {
            log::warn!("Workstation lock called on non-Windows platform (no-op)");
            Ok(())
        }
    }
}

/// Mock Windows locker for testing without locking the developer's workstation.
pub struct MockWindowsLocker {
    lock_count: AtomicU32,
    should_fail: AtomicBool,
}

impl MockWindowsLocker {
    pub fn new() -> Self {
        Self {
            lock_count: AtomicU32::new(0),
            should_fail: AtomicBool::new(false),
        }
    }

    pub fn lock_count(&self) -> u32 {
        self.lock_count.load(Ordering::Relaxed)
    }

    pub fn reset_count(&self) {
        self.lock_count.store(0, Ordering::Relaxed);
    }

    pub fn set_should_fail(&self, fail: bool) {
        self.should_fail.store(fail, Ordering::Relaxed);
    }
}

impl WorkstationLocker for MockWindowsLocker {
    fn lock_workstation(&self) -> Result<(), LockError> {
        if self.should_fail.load(Ordering::Relaxed) {
            return Err(LockError::ApiFailed("Simulated lock failure".to_string()));
        }
        self.lock_count.fetch_add(1, Ordering::Relaxed);
        log::info!("MockWindowsLocker: lock_workstation() invoked (count = {})", self.lock_count());
        Ok(())
    }
}
