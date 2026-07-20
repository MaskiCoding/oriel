//! Typed declarations for private SkyLight/CGS symbols, each resolved at runtime
//! via `dlsym` so a macOS release that drops one degrades a capability instead of
//! killing the process at load. Zero logic beyond resolution.

#![allow(non_snake_case)]
#![allow(clippy::pub_underscore_fields)] // fields are named exactly after the symbols they hold

use core::ffi::{c_char, c_int, c_void};

pub type CFTypeRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFStringRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Psn {
    pub hi: u32,
    pub lo: u32,
}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const SKYLIGHT_PATH: &[u8] = b"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight\0";
const RTLD_LAZY_LOCAL: c_int = 0x1 | 0x4;

unsafe fn resolve<T: Copy>(handle: *mut c_void, symbol: &'static str) -> Option<T> {
    debug_assert!(symbol.ends_with('\0'));
    debug_assert!(size_of::<T>() == size_of::<*mut c_void>());
    let ptr = unsafe { dlsym(handle, symbol.as_ptr().cast()) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { core::mem::transmute_copy(&ptr) })
    }
}

macro_rules! skylight {
    (
        required { $($rname:ident: fn($($rarg:ty),*) $(-> $rret:ty)?;)* }
        optional { $($oname:ident: fn($($oarg:ty),*) $(-> $oret:ty)?;)* }
    ) => {
        pub struct SkyLight {
            $(pub $rname: Option<unsafe extern "C" fn($($rarg),*) $(-> $rret)?>,)*
            $(pub $oname: Option<unsafe extern "C" fn($($oarg),*) $(-> $oret)?>,)*
        }

        impl SkyLight {
            /// Resolves every symbol from the `SkyLight` framework; `None` if the
            /// framework itself cannot be opened.
            pub fn load() -> Option<Self> {
                let handle = unsafe { dlopen(SKYLIGHT_PATH.as_ptr().cast(), RTLD_LAZY_LOCAL) };
                if handle.is_null() {
                    return None;
                }
                Some(Self {
                    $($rname: unsafe { resolve(handle, concat!(stringify!($rname), "\0")) },)*
                    $($oname: unsafe { resolve(handle, concat!(stringify!($oname), "\0")) },)*
                })
            }

            /// Required symbols that failed to resolve. Optional (capability)
            /// symbols are excluded: a missing one should degrade that one
            /// capability, not fail the whole connection.
            pub fn missing(&self) -> Vec<&'static str> {
                let mut names = Vec::new();
                $(if self.$rname.is_none() {
                    names.push(stringify!($rname));
                })*
                names
            }
        }
    };
}

skylight! {
    required {
        SLSMainConnectionID: fn() -> c_int;
        SLSCopyWindowsWithOptionsAndTags: fn(c_int, u32, CFArrayRef, u32, *mut u64, *mut u64) -> CFArrayRef;
        SLSCopyManagedDisplaySpaces: fn(c_int) -> CFArrayRef;
        SLSGetActiveSpace: fn(c_int) -> u64;
        SLSCopySpacesForWindows: fn(c_int, c_int, CFArrayRef) -> CFArrayRef;
        SLSWindowQueryWindows: fn(c_int, CFArrayRef, c_int) -> CFTypeRef;
        SLSWindowQueryResultCopyWindows: fn(CFTypeRef) -> CFTypeRef;
        SLSWindowIteratorGetCount: fn(CFTypeRef) -> c_int;
        SLSWindowIteratorAdvance: fn(CFTypeRef) -> bool;
        SLSWindowIteratorGetWindowID: fn(CFTypeRef) -> u32;
        SLSWindowIteratorGetParentID: fn(CFTypeRef) -> u32;
        SLSWindowIteratorGetPID: fn(CFTypeRef) -> c_int;
        SLSWindowIteratorGetTags: fn(CFTypeRef) -> u64;
        SLSWindowIteratorGetAttributes: fn(CFTypeRef) -> u64;
        SLSWindowIteratorGetLevel: fn(CFTypeRef) -> c_int;
        SLSCopyWindowProperty: fn(c_int, u32, CFStringRef, *mut CFTypeRef) -> c_int;
        CGSSetSymbolicHotKeyEnabled: fn(c_int, bool) -> c_int;
        CGSIsSymbolicHotKeyEnabled: fn(c_int) -> bool;
        _SLPSSetFrontProcessWithOptions: fn(*const Psn, u32, u32) -> c_int;
        SLPSPostEventRecordTo: fn(*const Psn, *const u8) -> c_int;
    }
    // Per-window screenshot, the only path that sees minimized/off-Space
    // windows. Two spellings of the same function; the `SLS` name is native to
    // SkyLight, `CGS` is a re-exported alias — resolve both, use whichever hits.
    optional {
        SLSHWCaptureWindowList: fn(c_int, *mut u32, u32, u32) -> CFArrayRef;
        CGSHWCaptureWindowList: fn(c_int, *mut u32, u32, u32) -> CFArrayRef;
    }
}
