use crate::state::AppState;
use std::path::Path;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

fn load_tauri_image(path: &Path) -> Option<tauri::image::Image<'static>> {
    if let Ok(img) = image::open(path) {
        let rgba = img.into_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        Some(tauri::image::Image::new_owned(rgba.into_raw(), w, h))
    } else {
        None
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<TrayIcon, tauri::Error> {
    let open_item = MenuItemBuilder::with_id("open", "Open FaceGuard").build(app)?;
    let enable_item = MenuItemBuilder::with_id("enable", "Enable Monitoring").build(app)?;
    let disable_item = MenuItemBuilder::with_id("disable", "Disable Monitoring (Kill Switch)").build(app)?;
    let lock_item = MenuItemBuilder::with_id("lock", "Lock Windows Now").build(app)?;
    let separator = MenuItemBuilder::with_id("sep", "---").build(app)?;
    let exit_item = MenuItemBuilder::with_id("exit", "Exit FaceGuard").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&open_item, &enable_item, &disable_item, &lock_item, &separator, &exit_item])
        .build()?;

    let icon_path = Path::new("resources/icons/tray_monitoring.png");
    let icon = load_tauri_image(icon_path)
        .unwrap_or_else(|| app.default_window_icon().unwrap().clone());

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("FaceGuard — Windows Face Presence Auto-Lock")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "open" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.unminimize();
                        let _ = win.set_focus();
                    }
                }
                "enable" => {
                    let state = app.state::<crate::app::AppContainer>();
                    state.controller.resume_monitoring();
                }
                "disable" => {
                    let state = app.state::<crate::app::AppContainer>();
                    state.controller.disable_monitoring();
                }
                "lock" => {
                    let state = app.state::<crate::app::AppContainer>();
                    let _ = state.controller.lock_now();
                }
                "exit" => {
                    let state = app.state::<crate::app::AppContainer>();
                    state.controller.shutdown();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(tray)
}

/// Updates tray icon based on current application state.
pub fn update_tray_icon_for_state(tray: &TrayIcon, state: AppState) {
    let icon_file = match state {
        AppState::Monitoring | AppState::FaceDetected => "tray_monitoring.png",
        AppState::Disabled => "tray_disabled.png",
        AppState::LockedStandby | AppState::LockRequested => "tray_locked.png",
        AppState::CameraError | AppState::FaceMissing => "tray_error.png",
        _ => "tray_monitoring.png",
    };

    let path = Path::new("resources/icons").join(icon_file);
    if let Some(img) = load_tauri_image(&path) {
        let _ = tray.set_icon(Some(img));
    }

    let tooltip = format!("FaceGuard: {state}");
    let _ = tray.set_tooltip(Some(&tooltip));
}
