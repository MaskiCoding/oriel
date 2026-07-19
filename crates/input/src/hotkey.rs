use core::ffi::c_void;

type OSStatus = i32;
type OSType = u32;
type EventRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventHotKeyRef = *mut c_void;

#[repr(C)]
struct EventTypeSpec {
    event_class: OSType,
    event_kind: u32,
}

#[repr(C)]
struct EventHotKeyID {
    signature: OSType,
    id: u32,
}

type EventHandlerProc =
    unsafe extern "C-unwind" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerProc,
        num_types: usize,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        handler_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RemoveEventHandler(handler_ref: EventHandlerRef) -> OSStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        hotkey_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hotkey_ref: EventHotKeyRef) -> OSStatus;
    fn GetEventParameter(
        event: EventRef,
        name: OSType,
        param_type: OSType,
        out_actual_type: *mut OSType,
        buffer_size: usize,
        out_actual_size: *mut usize,
        out_data: *mut c_void,
    ) -> OSStatus;
}

const fn fourcc(s: [u8; 4]) -> u32 {
    u32::from_be_bytes(s)
}

const K_EVENT_CLASS_KEYBOARD: OSType = fourcc(*b"keyb");
const K_EVENT_HOTKEY_PRESSED: u32 = 5;
const EVENT_NOT_HANDLED_ERR: OSStatus = -9874;
const K_EVENT_PARAM_DIRECT_OBJECT: OSType = fourcc(*b"----");
const TYPE_EVENT_HOTKEY_ID: OSType = fourcc(*b"hkid");
const SIGNATURE: OSType = fourcc(*b"orie");

pub const CMD: u32 = 1 << 8;
pub const SHIFT: u32 = 1 << 9;
pub const OPTION: u32 = 1 << 11;
pub const KEY_TAB: u32 = 48;

/// One trigger to bind: a key + modifier flags, tagged with the `id` reported
/// back when it fires.
pub struct Trigger {
    pub id: u32,
    pub key: u32,
    pub modifiers: u32,
}

type HotkeyFn = dyn FnMut(u32);

/// Carbon hot-key registrations plus their shared handler, all torn down on
/// drop. Carbon hot keys fire through Secure Input, unlike an event tap, and
/// consume the key combo so the focused app never sees it.
pub struct Hotkeys {
    handler: EventHandlerRef,
    registered: Vec<EventHotKeyRef>,
    callback: *mut Box<HotkeyFn>,
}

unsafe extern "C-unwind" fn handler_proc(
    _call: EventHandlerCallRef,
    event: EventRef,
    user_data: *mut c_void,
) -> OSStatus {
    let mut id = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            event,
            K_EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOTKEY_ID,
            core::ptr::null_mut(),
            size_of::<EventHotKeyID>(),
            core::ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status != 0 {
        return EVENT_NOT_HANDLED_ERR;
    }
    let callback = unsafe { &mut *user_data.cast::<Box<HotkeyFn>>() };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(id.id)));
    0
}

impl Hotkeys {
    /// Registers every `trigger`; `callback` receives each trigger's `id` when
    /// it fires. Returns `None` if the handler or any registration fails. Must
    /// be called on the main thread, where the run loop dispatches the handler.
    pub fn register(triggers: &[Trigger], callback: impl FnMut(u32) + 'static) -> Option<Self> {
        let callback: *mut Box<HotkeyFn> = Box::into_raw(Box::new(Box::new(callback)));
        let target = unsafe { GetApplicationEventTarget() };
        let spec = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOTKEY_PRESSED,
        };
        let mut handler: EventHandlerRef = core::ptr::null_mut();
        let status = unsafe {
            InstallEventHandler(
                target,
                handler_proc,
                1,
                &raw const spec,
                callback.cast(),
                &raw mut handler,
            )
        };
        if status != 0 {
            drop(unsafe { Box::from_raw(callback) });
            return None;
        }

        let mut this = Self {
            handler,
            registered: Vec::new(),
            callback,
        };
        for trigger in triggers {
            let id = EventHotKeyID {
                signature: SIGNATURE,
                id: trigger.id,
            };
            let mut hotkey: EventHotKeyRef = core::ptr::null_mut();
            let status = unsafe {
                RegisterEventHotKey(
                    trigger.key,
                    trigger.modifiers,
                    id,
                    target,
                    0,
                    &raw mut hotkey,
                )
            };
            if status != 0 {
                return None; // Drop unwinds the partial registration
            }
            this.registered.push(hotkey);
        }
        Some(this)
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for hotkey in self.registered.drain(..) {
            unsafe { UnregisterEventHotKey(hotkey) };
        }
        unsafe { RemoveEventHandler(self.handler) };
        drop(unsafe { Box::from_raw(self.callback) });
    }
}
