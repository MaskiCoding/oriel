use core::ffi::c_void;
use core::ptr::NonNull;

use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

/// A `CGEventMask` selecting the given event types (a set bit per type). Real
/// event types are all < 64; the `TapDisabled*` sentinels are not maskable.
pub fn event_mask(types: &[CGEventType]) -> CGEventMask {
    types.iter().fold(0, |mask, ty| {
        debug_assert!(ty.0 < 64, "not a maskable event type: {}", ty.0);
        mask | (1 << ty.0)
    })
}

/// Virtual keycode carried by a keyboard event.
pub fn keycode(event: &CGEvent) -> i64 {
    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode)
}

/// Modifier flags active on an event.
pub fn flags(event: &CGEvent) -> CGEventFlags {
    CGEvent::flags(Some(event))
}

/// What a tap callback decides for the event it just saw.
pub enum Disposition {
    /// Pass the event through untouched (the only option for a listen-only tap).
    Keep,
    /// Drop the event so nothing downstream receives it (absorbing taps only).
    Swallow,
}

type TapFn = dyn FnMut(CGEventType, &CGEvent) -> Disposition;

fn common_modes() -> Option<&'static objc2_core_foundation::CFRunLoopMode> {
    unsafe { kCFRunLoopCommonModes }
}

struct Ctx {
    callback: Box<TapFn>,
    port: *const CFMachPort,
}

/// A CoreGraphics event tap wired to a Rust closure, installed on the current
/// run loop and torn down on drop. The OS silently disables a tap that runs
/// long or is interrupted; the trampoline re-enables it in place.
///
/// The raw `ctx` pointer keeps this `!Send`, which is load-bearing: Drop frees
/// the callback, so it must run on the tap's own run-loop thread where no
/// callback can be executing. Do not add a manual `Send` impl.
pub struct EventTap {
    port: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
    ctx: *mut Ctx,
}

unsafe extern "C-unwind" fn trampoline(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    let ctx = unsafe { &mut *user_info.cast::<Ctx>() };
    if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
        if let Some(port) = unsafe { ctx.port.as_ref() } {
            CGEvent::tap_enable(port, true);
        }
        return event.as_ptr();
    }
    // The trampoline is `C-unwind`, so a panic here would unwind into CoreGraphics.
    // Contain it and pass the event through rather than corrupt the tap dispatch.
    let disposition = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (ctx.callback)(ty, unsafe { event.as_ref() })
    }));
    match disposition {
        Ok(Disposition::Swallow) => core::ptr::null_mut(),
        Ok(Disposition::Keep) | Err(_) => event.as_ptr(),
    }
}

impl EventTap {
    /// Installs a session-level tap for `events`. Returns `None` when the OS
    /// refuses the tap — most often because Accessibility/Input-Monitoring is
    /// not granted to this binary.
    pub fn install(
        options: CGEventTapOptions,
        events: CGEventMask,
        callback: impl FnMut(CGEventType, &CGEvent) -> Disposition + 'static,
    ) -> Option<Self> {
        let ctx = Box::into_raw(Box::new(Ctx {
            callback: Box::new(callback),
            port: core::ptr::null(),
        }));
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                options,
                events,
                Some(trampoline),
                ctx.cast(),
            )
        };
        let Some(port) = port else {
            drop(unsafe { Box::from_raw(ctx) });
            return None;
        };
        unsafe { (*ctx).port = CFRetained::as_ptr(&port).as_ptr() };

        let install = || {
            let source = CFMachPort::new_run_loop_source(None, Some(&port), 0)?;
            let run_loop = CFRunLoop::current()?;
            run_loop.add_source(Some(&source), common_modes());
            CGEvent::tap_enable(&port, true);
            Some((source, run_loop))
        };
        let Some((source, run_loop)) = install() else {
            drop(unsafe { Box::from_raw(ctx) });
            return None;
        };
        Some(Self {
            port,
            source,
            run_loop,
            ctx,
        })
    }

    /// A cheap handle for enabling and disabling this tap from elsewhere. It
    /// borrows nothing from the tap's closure, so holding one alongside the
    /// callback's own captures cannot form a reference cycle.
    pub fn handle(&self) -> TapHandle {
        TapHandle(self.port.clone())
    }
}

/// Enables or disables an installed [`EventTap`] without owning it.
pub struct TapHandle(CFRetained<CFMachPort>);

impl TapHandle {
    pub fn set_enabled(&self, enabled: bool) {
        CGEvent::tap_enable(&self.0, enabled);
    }
}

impl Drop for EventTap {
    fn drop(&mut self) {
        CGEvent::tap_enable(&self.port, false);
        self.run_loop
            .remove_source(Some(&self.source), common_modes());
        drop(unsafe { Box::from_raw(self.ctx) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_sets_one_bit_per_type() {
        assert_eq!(event_mask(&[]), 0);
        assert_eq!(event_mask(&[CGEventType::KeyDown]), 1 << 10);
        assert_eq!(
            event_mask(&[CGEventType::KeyDown, CGEventType::FlagsChanged]),
            (1 << 10) | (1 << 12),
        );
    }
}
