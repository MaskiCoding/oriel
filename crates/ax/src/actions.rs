use core::ffi::c_void;
use core::ptr::NonNull;

use objc2_core_foundation::{CFBoolean, CFRetained, CFString, kCFBooleanFalse, kCFBooleanTrue};

use crate::element::{
    AX_SUCCESS, AXUIElementCopyAttributeValue, AXUIElementPerformAction,
    AXUIElementSetAttributeValue, AxRef, CFRelease, app_element, as_ptr, with_window,
};

/// Presses the window's close button via Accessibility. Returns false if the
/// window, button, or press action is unavailable.
pub fn close_window(pid: i32, wid: u32) -> bool {
    let Some(app) = app_element(pid) else {
        return false;
    };
    let closed = with_window(app, wid, close_element).unwrap_or(false);
    unsafe { CFRelease(app) };
    closed
}

fn close_element(element: AxRef) -> bool {
    let attr = CFString::from_str("AXCloseButton");
    let mut value: *const c_void = core::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(element, as_ptr(&attr), &raw mut value) };
    let Some(value) = NonNull::new(value.cast_mut()) else {
        return false;
    };
    if err != AX_SUCCESS {
        unsafe { CFRelease(value.as_ptr()) };
        return false;
    }
    let press = CFString::from_str("AXPress");
    let press_err = unsafe { AXUIElementPerformAction(value.as_ptr(), as_ptr(&press)) };
    unsafe { CFRelease(value.as_ptr()) };
    press_err == AX_SUCCESS
}

pub fn set_minimized(pid: i32, wid: u32, minimized: bool) -> bool {
    set_window_bool(pid, wid, "AXMinimized", minimized)
}

pub fn is_minimized(pid: i32, wid: u32) -> Option<bool> {
    get_window_bool(pid, wid, "AXMinimized")
}

pub fn set_fullscreen(pid: i32, wid: u32, fullscreen: bool) -> bool {
    set_window_bool(pid, wid, "AXFullScreen", fullscreen)
}

pub fn is_fullscreen(pid: i32, wid: u32) -> Option<bool> {
    get_window_bool(pid, wid, "AXFullScreen")
}

pub fn set_app_hidden(pid: i32, hidden: bool) -> bool {
    let Some(app) = app_element(pid) else {
        return false;
    };
    let ok = set_bool_attr(app, "AXHidden", hidden);
    unsafe { CFRelease(app) };
    ok
}

pub fn is_app_hidden(pid: i32) -> Option<bool> {
    let app = app_element(pid)?;
    let value = get_bool_attr(app, "AXHidden");
    unsafe { CFRelease(app) };
    value
}

fn set_window_bool(pid: i32, wid: u32, attr: &str, value: bool) -> bool {
    let Some(app) = app_element(pid) else {
        return false;
    };
    let ok = with_window(app, wid, |element| set_bool_attr(element, attr, value)).unwrap_or(false);
    unsafe { CFRelease(app) };
    ok
}

fn get_window_bool(pid: i32, wid: u32, attr: &str) -> Option<bool> {
    let app = app_element(pid)?;
    let value = with_window(app, wid, |element| get_bool_attr(element, attr)).flatten();
    unsafe { CFRelease(app) };
    value
}

fn set_bool_attr(element: AxRef, name: &str, value: bool) -> bool {
    let attr = CFString::from_str(name);
    let Some(cf) = (if value {
        unsafe { kCFBooleanTrue }
    } else {
        unsafe { kCFBooleanFalse }
    }) else {
        return false;
    };
    let err = unsafe {
        AXUIElementSetAttributeValue(element, as_ptr(&attr), core::ptr::from_ref(cf).cast())
    };
    err == AX_SUCCESS
}

fn get_bool_attr(element: AxRef, name: &str) -> Option<bool> {
    let attr = CFString::from_str(name);
    let mut value: *const c_void = core::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(element, as_ptr(&attr), &raw mut value) };
    let value = NonNull::new(value.cast_mut())?;
    if err != AX_SUCCESS {
        unsafe { CFRelease(value.as_ptr()) };
        return None;
    }
    let boolean: CFRetained<CFBoolean> = unsafe { CFRetained::from_raw(value.cast()) };
    Some(boolean.as_bool())
}
