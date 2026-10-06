//! Side mouse buttons on macOS never arrive in the webview. AppKit takes
//! button 3 (back) and button 4 (forward) before the page can see them, so
//! the shell watches for them and tells the page to walk its own history.

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use tauri::Emitter;

pub fn install(app: &tauri::App) {
    let handle = app.handle().clone();
    let block = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let ptr = event.as_ptr();
        let event = unsafe { event.as_ref() };
        let button = event.buttonNumber();
        if button != 3 && button != 4 {
            return ptr;
        }
        if event.r#type() == NSEventType::OtherMouseDown {
            let step: i8 = if button == 3 { -1 } else { 1 };
            let _ = handle.emit("mouse-history", step);
        }
        std::ptr::null_mut()
    });
    // The monitor stays for the life of the process. Releasing it removes the watch.
    let Some(monitor) = (unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::OtherMouseDown | NSEventMask::OtherMouseUp,
            &block,
        )
    }) else {
        tracing::error!("side mouse buttons could not be watched");
        return;
    };
    std::mem::forget(monitor);
    std::mem::forget(block);
}
