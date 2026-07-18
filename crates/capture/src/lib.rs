//! Preview pipeline: `ScreenCaptureKit` captures, private fallback for minimized/off-Space windows.

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
