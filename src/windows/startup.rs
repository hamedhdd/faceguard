#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
#[cfg(windows)]
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_REG_NAME: &str = "FaceGuard";

/// Registers or unregisters FaceGuard to launch at Windows login for current user.
pub fn set_run_at_startup(enable: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE | KEY_READ)
            .map_err(|e| format!("Failed to open registry Run key: {e}"))?;

        if enable {
            let current_exe = std::env::current_exe()
                .map_err(|e| format!("Failed to determine current executable path: {e}"))?;
            let path_str = current_exe.to_str().ok_or("Invalid UTF-8 in exe path")?;
            let quoted_path = format!("\"{path_str}\" --minimized");

            key.set_value(APP_REG_NAME, &quoted_path)
                .map_err(|e| format!("Failed to set registry value: {e}"))?;
            log::info!("Registered startup entry: {quoted_path}");
        } else {
            let _ = key.delete_value(APP_REG_NAME);
            log::info!("Removed startup registry entry");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enable;
        Ok(())
    }
}

/// Checks whether FaceGuard is currently registered to start with Windows.
pub fn is_run_at_startup_enabled() -> bool {
    #[cfg(windows)]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_READ) {
            let val: Result<String, _> = key.get_value(APP_REG_NAME);
            val.is_ok()
        } else {
            false
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}
