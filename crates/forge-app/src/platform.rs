//! The only OS-specific code in the app: window material, notifications, and OS file drops
//! (Slint's winit backend does not forward them yet, so we tap winit directly).
use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::winit_030::{WinitWindowAccessor, winit::event::WindowEvent, EventResult};

use crate::{App, State};

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

/// Files dragged from Explorer / Finder / a file manager onto the window. winit delivers one
/// `DroppedFile` per path, then `HoveredFileCancelled`; we batch until the last one lands.
pub fn hook_os_drops(ui: &App, st: Rc<RefCell<State>>) {
    let weak = ui.as_weak();
    ui.window().on_winit_window_event(move |_, ev| {
        match ev {
            WindowEvent::HoveredFile(_) => weak.unwrap().set_drag_over(true),
            WindowEvent::HoveredFileCancelled => weak.unwrap().set_drag_over(false),
            WindowEvent::DroppedFile(p) => {
                let ui = weak.unwrap();
                ui.set_drag_over(false);
                st.borrow_mut().dropping.push(p.clone());
                // Coalesce the burst: winit sends N DroppedFile events back to back in one turn of the loop.
                let (w, st) = (weak.clone(), st.clone());
                slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
                    let pending = std::mem::take(&mut st.borrow_mut().dropping);
                    if !pending.is_empty() {
                        crate::add(&w.unwrap(), &st, pending);
                    }
                });
            }
            _ => return EventResult::Propagate,
        }
        EventResult::Propagate
    });
}

pub fn notify(title: &str, body: &str) {
    let _ = notify_rust::Notification::new().summary(title).body(body).appname("Husky Forge").show();
}
