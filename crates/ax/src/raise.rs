use core::ffi::c_void;
use core::ptr::NonNull;

use objc2_core_foundation::{CFString, kCFBooleanFalse};

use crate::element::{
    _AXUIElementGetWindow, AX_SUCCESS, AXUIElementCopyAttributeValue, AXUIElementCreateApplication,
    AXUIElementPerformAction, AXUIElementSetAttributeValue, AxRef, CFRelease, as_ptr, with_window,
};

/// De-minimizes (if needed) and raises window `wid` of process `pid` to the
/// front of its app's window stack via Accessibility.
/// `_SLPSSetFrontProcessWithOptions` fronts the process but leaves the window
/// behind — and can't reach a minimized one at all; this handles both. Needs
/// Accessibility trust.
pub fn raise_window(pid: i32, wid: u32) -> bool {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return false;
    }
    let raised = raise_matching(app, wid);
    unsafe { CFRelease(app) };
    raised
}

/// The window id of `pid`'s currently focused window, via Accessibility —
/// lets us record where the user actually is, however they got there (a click,
/// an app's own shortcut), not just Oriel's own switches.
pub fn focused_window(pid: i32) -> Option<u32> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return None;
    }
    let wid = focused_wid(app);
    unsafe { CFRelease(app) };
    wid
}

fn focused_wid(app: AxRef) -> Option<u32> {
    let attr = CFString::from_str("AXFocusedWindow");
    let mut value: *const c_void = core::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(app, as_ptr(&attr), &raw mut value) };
    let value = NonNull::new(value.cast_mut())?;
    if err != AX_SUCCESS {
        unsafe { CFRelease(value.as_ptr()) };
        return None;
    }
    let mut wid = 0u32;
    let found = unsafe { _AXUIElementGetWindow(value.as_ptr(), &raw mut wid) } == AX_SUCCESS;
    unsafe { CFRelease(value.as_ptr()) };
    found.then_some(wid)
}

fn raise_matching(app: AxRef, wid: u32) -> bool {
    let minimized_attr = CFString::from_str("AXMinimized");
    let raise_action = CFString::from_str("AXRaise");
    with_window(app, wid, |element| {
        if let Some(no) = unsafe { kCFBooleanFalse } {
            unsafe {
                AXUIElementSetAttributeValue(
                    element,
                    as_ptr(&minimized_attr),
                    core::ptr::from_ref(no).cast(),
                );
            }
        }
        let raise_err = unsafe { AXUIElementPerformAction(element, as_ptr(&raise_action)) };
        raise_err == AX_SUCCESS
    })
    .unwrap_or(false)
}
