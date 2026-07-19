//! `WindowServer` integration: enumeration, batched state queries, event tap, Space topology.

mod focus;

use core::ffi::{c_int, c_void};
use core::ptr::NonNull;

use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, Type};
use skylight_sys::SkyLight;

unsafe extern "C" {
    fn proc_name(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
}

pub struct SpaceInfo {
    pub id: u64,
    pub current: bool,
}

pub struct WindowInfo {
    pub wid: u32,
    pub pid: i32,
    pub parent: u32,
    pub level: i32,
    pub tags: u64,
    pub attributes: u64,
    pub app: Option<String>,
    pub title: Option<String>,
}

pub struct WindowServer {
    sl: SkyLight,
    cid: c_int,
}

impl WindowServer {
    /// Connects to the `WindowServer`; `Err` lists whatever failed to resolve.
    pub fn connect() -> Result<Self, Vec<&'static str>> {
        let Some(sl) = SkyLight::load() else {
            return Err(vec!["SkyLight framework"]);
        };
        let missing = sl.missing();
        if !missing.is_empty() {
            return Err(missing);
        }
        let cid = unsafe { sl.SLSMainConnectionID.unwrap()() };
        Ok(Self { sl, cid })
    }

    pub fn spaces(&self) -> Vec<SpaceInfo> {
        let mut spaces = Vec::new();
        let raw = unsafe { self.sl.SLSCopyManagedDisplaySpaces.unwrap()(self.cid) };
        let Some(displays) = (unsafe { retained::<CFArray>(raw) }) else {
            return spaces;
        };
        for i in 0..displays.count() {
            let Some(display) = (unsafe { element::<CFDictionary>(&displays, i) }) else {
                continue;
            };
            let current = dict_dict(display, "Current Space").and_then(|d| dict_i64(d, "id64"));
            let Some(list) = dict_array(display, "Spaces") else {
                continue;
            };
            for j in 0..list.count() {
                let Some(space) = (unsafe { element::<CFDictionary>(list, j) }) else {
                    continue;
                };
                if let Some(id) = dict_i64(space, "id64") {
                    spaces.push(SpaceInfo {
                        id: id.cast_unsigned(),
                        current: current == Some(id),
                    });
                }
            }
        }
        spaces
    }

    pub fn windows(&self, space_ids: &[u64]) -> Vec<WindowInfo> {
        let ids: Vec<CFRetained<CFNumber>> = space_ids
            .iter()
            .map(|&id| CFNumber::new_i64(id.cast_signed()))
            .collect();
        let spaces = CFArray::from_retained_objects(&ids);
        let mut set_tags = 0_u64;
        let mut clear_tags = 0_u64;
        let raw = unsafe {
            self.sl.SLSCopyWindowsWithOptionsAndTags.unwrap()(
                self.cid,
                0,
                raw_ptr(&spaces),
                0x2,
                &raw mut set_tags,
                &raw mut clear_tags,
            )
        };
        let Some(list) = (unsafe { retained::<CFArray>(raw) }) else {
            return Vec::new();
        };
        let count = i32::try_from(list.count()).unwrap_or(0);
        let raw =
            unsafe { self.sl.SLSWindowQueryWindows.unwrap()(self.cid, raw_ptr(&list), count) };
        let Some(query) = (unsafe { retained::<CFType>(raw) }) else {
            return Vec::new();
        };
        let raw = unsafe { self.sl.SLSWindowQueryResultCopyWindows.unwrap()(raw_ptr(&query)) };
        let Some(iter) = (unsafe { retained::<CFType>(raw) }) else {
            return Vec::new();
        };
        let mut windows = Vec::new();
        while unsafe { self.sl.SLSWindowIteratorAdvance.unwrap()(raw_ptr(&iter)) } {
            let pid = unsafe { self.sl.SLSWindowIteratorGetPID.unwrap()(raw_ptr(&iter)) };
            let wid = unsafe { self.sl.SLSWindowIteratorGetWindowID.unwrap()(raw_ptr(&iter)) };
            windows.push(WindowInfo {
                wid,
                pid,
                parent: unsafe { self.sl.SLSWindowIteratorGetParentID.unwrap()(raw_ptr(&iter)) },
                level: unsafe { self.sl.SLSWindowIteratorGetLevel.unwrap()(raw_ptr(&iter)) },
                tags: unsafe { self.sl.SLSWindowIteratorGetTags.unwrap()(raw_ptr(&iter)) },
                attributes: unsafe {
                    self.sl.SLSWindowIteratorGetAttributes.unwrap()(raw_ptr(&iter))
                },
                app: app_name(pid),
                title: self.window_title(wid),
            });
        }
        windows
    }

    fn window_title(&self, wid: u32) -> Option<String> {
        let key = CFString::from_str("kCGSWindowTitle");
        let mut out: skylight_sys::CFTypeRef = core::ptr::null();
        let err = unsafe {
            self.sl.SLSCopyWindowProperty.unwrap()(self.cid, wid, raw_ptr(&key), &raw mut out)
        };
        if err != 0 {
            return None;
        }
        let title = unsafe { retained::<CFString>(out) }?;
        Some(title.to_string())
    }
}

fn app_name(pid: i32) -> Option<String> {
    let mut buf = [0_u8; 64];
    let len = unsafe { proc_name(pid, buf.as_mut_ptr().cast(), 64) };
    let len = usize::try_from(len).ok().filter(|&n| n > 0)?;
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

fn raw_ptr<T: ?Sized + Type>(r: &CFRetained<T>) -> *const c_void {
    CFRetained::as_ptr(r).as_ptr().cast_const().cast()
}

unsafe fn retained<T: Type>(raw: *const c_void) -> Option<CFRetained<T>> {
    let ptr = NonNull::new(raw.cast_mut())?;
    Some(unsafe { CFRetained::from_raw(ptr.cast()) })
}

unsafe fn element<T>(array: &CFArray, index: isize) -> Option<&T> {
    let ptr = NonNull::new(unsafe { array.value_at_index(index) }.cast_mut())?;
    Some(unsafe { ptr.cast().as_ref() })
}

fn dict_value(dict: &CFDictionary, key: &str) -> Option<NonNull<c_void>> {
    let key = CFString::from_str(key);
    NonNull::new(unsafe { dict.value(raw_ptr(&key)) }.cast_mut())
}

fn dict_dict<'a>(dict: &'a CFDictionary, key: &str) -> Option<&'a CFDictionary> {
    dict_value(dict, key).map(|p| unsafe { p.cast().as_ref() })
}

fn dict_array<'a>(dict: &'a CFDictionary, key: &str) -> Option<&'a CFArray> {
    dict_value(dict, key).map(|p| unsafe { p.cast().as_ref() })
}

fn dict_i64(dict: &CFDictionary, key: &str) -> Option<i64> {
    dict_value(dict, key).and_then(|p| unsafe { p.cast::<CFNumber>().as_ref() }.as_i64())
}
