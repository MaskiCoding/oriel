use core::ffi::c_void;
use core::ptr::NonNull;

use objc2_core_foundation::{CFArray, CFRetained, CFString};

type AxRef = *const c_void;
const AX_SUCCESS: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AxRef;
    fn AXUIElementCopyAttributeValue(
        element: AxRef,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementPerformAction(element: AxRef, action: *const c_void) -> i32;
    fn _AXUIElementGetWindow(element: AxRef, wid: *mut u32) -> i32;
    fn CFRelease(cf: *const c_void);
}

fn as_ptr(s: &CFString) -> *const c_void {
    core::ptr::from_ref(s).cast()
}

/// Raises window `wid` of process `pid` to the front of its app's window stack
/// via Accessibility. `_SLPSSetFrontProcessWithOptions` fronts the process but
/// leaves the window behind; this brings it forward. Needs Accessibility trust.
pub fn raise_window(pid: i32, wid: u32) -> bool {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return false;
    }
    let raised = raise_matching(app, wid);
    unsafe { CFRelease(app) };
    raised
}

fn raise_matching(app: AxRef, wid: u32) -> bool {
    let windows_attr = CFString::from_str("AXWindows");
    let mut value: *const c_void = core::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(app, as_ptr(&windows_attr), &raw mut value) };
    let Some(value) = NonNull::new(value.cast_mut()) else {
        return false;
    };
    if err != AX_SUCCESS {
        unsafe { CFRelease(value.as_ptr()) };
        return false;
    }
    let windows: CFRetained<CFArray> = unsafe { CFRetained::from_raw(value.cast()) };
    let raise_action = CFString::from_str("AXRaise");
    for i in 0..windows.count() {
        let element = unsafe { windows.value_at_index(i) };
        if element.is_null() {
            continue;
        }
        let mut ewid: u32 = 0;
        if unsafe { _AXUIElementGetWindow(element, &raw mut ewid) } == AX_SUCCESS && ewid == wid {
            unsafe { AXUIElementPerformAction(element, as_ptr(&raise_action)) };
            return true;
        }
    }
    false
}
