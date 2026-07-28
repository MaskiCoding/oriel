use core::ffi::c_void;
use core::ptr::NonNull;

use objc2_core_foundation::{CFArray, CFRetained, CFString};

pub(crate) type AxRef = *const c_void;
pub(crate) const AX_SUCCESS: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn AXUIElementCreateApplication(pid: i32) -> AxRef;
    pub(crate) fn AXUIElementCopyAttributeValue(
        element: AxRef,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    pub(crate) fn AXUIElementSetAttributeValue(
        element: AxRef,
        attribute: *const c_void,
        value: *const c_void,
    ) -> i32;
    pub(crate) fn AXUIElementPerformAction(element: AxRef, action: *const c_void) -> i32;
    pub(crate) fn _AXUIElementGetWindow(element: AxRef, wid: *mut u32) -> i32;
    pub(crate) fn CFRelease(cf: *const c_void);
}

pub(crate) fn as_ptr(s: &CFString) -> *const c_void {
    core::ptr::from_ref(s).cast()
}

/// Finds the AX element for window `wid` among `app`'s `AXWindows` and runs
/// `f` while the windows array (and thus the element) is still retained.
pub(crate) fn with_window<R>(app: AxRef, wid: u32, f: impl FnOnce(AxRef) -> R) -> Option<R> {
    let windows_attr = CFString::from_str("AXWindows");
    let mut value: *const c_void = core::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(app, as_ptr(&windows_attr), &raw mut value) };
    let value = NonNull::new(value.cast_mut())?;
    if err != AX_SUCCESS {
        unsafe { CFRelease(value.as_ptr()) };
        return None;
    }
    let windows: CFRetained<CFArray> = unsafe { CFRetained::from_raw(value.cast()) };
    for i in 0..windows.count() {
        let element = unsafe { windows.value_at_index(i) };
        if element.is_null() {
            continue;
        }
        let mut ewid: u32 = 0;
        if unsafe { _AXUIElementGetWindow(element, &raw mut ewid) } == AX_SUCCESS && ewid == wid {
            return Some(f(element));
        }
    }
    None
}
