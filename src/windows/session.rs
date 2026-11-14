use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    Locked,
    Unlocked,
}

pub type SessionCallback = Arc<dyn Fn(SessionEvent) + Send + Sync + 'static>;

pub trait SessionNotifier: Send + Sync {
    /// Starts listening for Windows session lock/unlock events.
    fn start(&self, callback: SessionCallback) -> Result<(), String>;
    /// Stops listening and unregisters notifications.
    fn stop(&self);
}

/// Production Windows session notification listener using WTSRegisterSessionNotification.
pub struct WindowsSessionNotifier {
    is_running: AtomicBool,
    stop_handle: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl WindowsSessionNotifier {
    pub fn new() -> Self {
        Self {
            is_running: AtomicBool::new(false),
            stop_handle: Mutex::new(None),
        }
    }
}

#[cfg(windows)]
mod win_impl {
    use super::*;
    use std::ptr::null_mut;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        PostMessageW, RegisterClassExW, SetWindowLongPtrW, GetWindowLongPtrW,
        GWLP_USERDATA, MSG, WNDCLASSEXW, WM_DESTROY,
    };

    const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;
    const WM_APP_STOP: u32 = 0x8001;

    struct Context {
        callback: SessionCallback,
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_WTSSESSION_CHANGE => {
                let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
                if !ctx_ptr.is_null() {
                    let ctx = &*ctx_ptr;
                    match wparam.0 {
                        WTS_SESSION_LOCK => {
                            log::info!("Native Windows Session Notification: WTS_SESSION_LOCK received");
                            (ctx.callback)(SessionEvent::Locked);
                        }
                        WTS_SESSION_UNLOCK => {
                            log::info!("Native Windows Session Notification: WTS_SESSION_UNLOCK received");
                            (ctx.callback)(SessionEvent::Unlocked);
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_APP_STOP => {
                let _ = WTSUnRegisterSessionNotification(hwnd);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn start_listener(
        callback: SessionCallback,
    ) -> Result<(std::thread::JoinHandle<()>, isize), String> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        let handle = std::thread::Builder::new()
            .name("faceguard-session-listener".to_string())
            .spawn(move || {
                unsafe {
                    let h_instance = match GetModuleHandleW(PCWSTR(null_mut())) {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = tx.send(Err(format!("GetModuleHandleW failed: {e}")));
                            return;
                        }
                    };

                    let class_name = w!("FaceGuard_SessionNotificationWindow");
                    let mut wc = WNDCLASSEXW::default();
                    wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                    wc.lpfnWndProc = Some(window_proc);
                    wc.hInstance = h_instance.into();
                    wc.lpszClassName = class_name;

                    // Register window class
                    let _ = RegisterClassExW(&wc);

                    // Create message-only window
                    let hwnd = match CreateWindowExW(
                        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                        class_name,
                        w!("FaceGuardSessionListener"),
                        windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(h_instance.into()),
                        None,
                    ) {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = tx.send(Err(format!("CreateWindowExW failed: {e}")));
                            return;
                        }
                    };

                    let mut context = Box::new(Context { callback });
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *context as *mut Context as isize);

                    // Register for Windows session notifications
                    if let Err(e) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
                        let _ = tx.send(Err(format!("WTSRegisterSessionNotification failed: {e}")));
                        let _ = DestroyWindow(hwnd);
                        return;
                    }

                    log::info!("WTSRegisterSessionNotification registered successfully on HWND {:?}", hwnd);
                    let hwnd_val = hwnd.0 as isize;
                    let _ = tx.send(Ok(hwnd_val));

                    // Win32 message pump
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        DispatchMessageW(&msg);
                        if msg.message == WM_DESTROY {
                            break;
                        }
                    }

                    log::info!("Session listener message pump terminated");
                }
            })
            .map_err(|e| e.to_string())?;

        let hwnd_val = rx.recv().map_err(|e| e.to_string())??;
        Ok((handle, hwnd_val))
    }

    pub fn stop_listener(hwnd_val: isize) {
        unsafe {
            let hwnd = HWND(hwnd_val as *mut core::ffi::c_void);
            let _ = PostMessageW(Some(hwnd), WM_APP_STOP, WPARAM(0), LPARAM(0));
        }
    }
}

impl SessionNotifier for WindowsSessionNotifier {
    fn start(&self, callback: SessionCallback) -> Result<(), String> {
        #[cfg(windows)]
        {
            if self.is_running.swap(true, Ordering::SeqCst) {
                return Ok(()); // Already running
            }

            let (_handle, hwnd_val) = win_impl::start_listener(callback)?;
            *self.stop_handle.lock() = Some(Box::new(move || {
                win_impl::stop_listener(hwnd_val);
            }));

            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = callback;
            log::warn!("WindowsSessionNotifier not supported on non-Windows platforms");
            Ok(())
        }
    }

    fn stop(&self) {
        if self.is_running.swap(false, Ordering::SeqCst) {
            if let Some(stop_fn) = self.stop_handle.lock().take() {
                stop_fn();
            }
        }
    }
}

impl Drop for WindowsSessionNotifier {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Mock session notifier for automated testing.
pub struct MockSessionNotifier {
    callback: Mutex<Option<SessionCallback>>,
    is_running: AtomicBool,
}

impl MockSessionNotifier {
    pub fn new() -> Self {
        Self {
            callback: Mutex::new(None),
            is_running: AtomicBool::new(false),
        }
    }

    pub fn simulate_lock(&self) {
        if let Some(cb) = self.callback.lock().as_ref() {
            log::info!("MockSessionNotifier: Simulating SessionEvent::Locked");
            cb(SessionEvent::Locked);
        }
    }

    pub fn simulate_unlock(&self) {
        if let Some(cb) = self.callback.lock().as_ref() {
            log::info!("MockSessionNotifier: Simulating SessionEvent::Unlocked");
            cb(SessionEvent::Unlocked);
        }
    }
}

impl SessionNotifier for MockSessionNotifier {
    fn start(&self, callback: SessionCallback) -> Result<(), String> {
        *self.callback.lock() = Some(callback);
        self.is_running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        *self.callback.lock() = None;
    }
}
