//! Preview pipeline: window screenshots for the strip tiles. The primary path is
//! the `WindowServer`'s hardware capture, the only one that also sees minimized
//! and off-Space windows.

use core::ffi::c_int;
use core::ptr::NonNull;

use objc2_core_foundation::{CFArray, CFRetained};
use objc2_core_graphics::CGImage;
use skylight_sys::{CFArrayRef, SkyLight};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn permitted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Shows the system Screen Recording prompt when not granted; returns the grant state.
pub fn request_permission() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

/// `CGSWindowCaptureOptions` for a clean, full-window grab: best (backing-store)
/// resolution, ignore the global clip shape, and force full size — the last
/// avoids skewed captures when Stage Manager is on.
const CAPTURE_OPTIONS: u32 = 0x100 | 0x800 | 0x8_0000;

type CaptureFn = unsafe extern "C" fn(c_int, *mut u32, u32, u32) -> CFArrayRef;

/// Grabs window screenshots straight from the `WindowServer`, over one
/// connection reused for every capture. Safe to call off the main thread.
pub struct Capturer {
    capture: CaptureFn,
    cid: c_int,
}

impl Capturer {
    /// `None` if the capture symbol or the `WindowServer` connection can't be
    /// resolved; the caller then runs preview-less (Icons/List) mode.
    pub fn new() -> Option<Self> {
        let sl = SkyLight::load()?;
        let capture = sl.SLSHWCaptureWindowList.or(sl.CGSHWCaptureWindowList)?;
        let cid = unsafe { sl.SLSMainConnectionID?() };
        Some(Self { capture, cid })
    }

    /// A screenshot of `wid` — works for minimized and off-Space windows.
    /// `None` when the window has nothing to show (zero size, gone). Without
    /// Screen Recording permission the frame is black, so gate on [`permitted`].
    pub fn window_image(&self, wid: u32) -> Option<CFRetained<CGImage>> {
        let mut ids = [wid];
        let raw = unsafe { (self.capture)(self.cid, ids.as_mut_ptr(), 1, CAPTURE_OPTIONS) };
        // Returned +1: owning it here releases it (and its images) on drop.
        let array =
            unsafe { CFRetained::<CFArray>::from_raw(NonNull::new(raw.cast_mut())?.cast()) };
        if array.count() < 1 {
            return None;
        }
        // The image is owned by the array, so retain it to outlive the drop.
        let image = NonNull::new(unsafe { array.value_at_index(0) }.cast_mut())?;
        Some(unsafe { CFRetained::<CGImage>::retain(image.cast()) })
    }
}
