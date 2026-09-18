//! The only OS-specific code in the app: window material and notifications.
use slint::ComponentHandle;

use crate::App;

/// Windows 11 Mica / macOS vibrancy; everything else keeps the opaque themed background.
pub fn decorate(ui: &App) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let handle = ui.window().window_handle();
        #[cfg(windows)]
        let ok = window_vibrancy::apply_mica(&handle, None).is_ok();
        #[cfg(target_os = "macos")]
        let ok = window_vibrancy::apply_vibrancy(&handle, window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground, None, None).is_ok();
        ui.set_transparent_backdrop(ok);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = ui;
}

pub fn notify(title: &str, body: &str) {
    let _ = notify_rust::Notification::new().summary(title).body(body).appname("Husky Forge").show();
}
