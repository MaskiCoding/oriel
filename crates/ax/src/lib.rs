//! Safe RAII wrapper over the C Accessibility API.

mod actions;
mod element;
mod raise;

pub use actions::{
    close_window, is_app_hidden, is_fullscreen, is_minimized, set_app_hidden, set_fullscreen,
    set_minimized,
};
pub use raise::{focused_window, raise_window};

use core::ffi::c_void;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFString, kCFBooleanTrue};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: Option<&'static CFString>;
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Like [`trusted`], but shows the system Accessibility prompt when not granted.
pub fn request_trust() -> bool {
    let (Some(key), Some(yes)) = (unsafe { kAXTrustedCheckOptionPrompt }, unsafe {
        kCFBooleanTrue
    }) else {
        return trusted();
    };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[yes]);
    let ptr = core::ptr::from_ref::<CFDictionary<CFString, CFBoolean>>(&options);
    unsafe { AXIsProcessTrustedWithOptions(ptr.cast()) }
}
