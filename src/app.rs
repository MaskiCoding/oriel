//! The M1 loop: hold a trigger, cycle with taps, release to jump.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_core_graphics::{CGEventFlags, CGEventTapOptions, CGEventType};

/// Runs `work` on the next main-run-loop turn. The `WindowServer` ignores an
/// activation attempted from inside the hot-key handler that consumed the
/// triggering event, so the focus jump is deferred out of that context.
fn on_next_turn(work: impl FnOnce() + 'static) {
    use core::ffi::c_void;
    #[repr(C)]
    struct Queue([u8; 0]);
    unsafe extern "C" {
        static _dispatch_main_q: Queue;
        fn dispatch_async_f(
            queue: *const Queue,
            context: *mut c_void,
            work: unsafe extern "C" fn(*mut c_void),
        );
    }
    unsafe extern "C" fn call(ctx: *mut c_void) {
        let work: Box<Box<dyn FnOnce()>> = unsafe { Box::from_raw(ctx.cast()) };
        // `call` is plain `extern "C"`, so a panic must not unwind into libdispatch.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    }
    let boxed: Box<Box<dyn FnOnce()>> = Box::new(Box::new(work));
    unsafe {
        dispatch_async_f(
            &raw const _dispatch_main_q,
            Box::into_raw(boxed).cast(),
            call,
        );
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Modifier {
    Command,
    Option,
}

impl Modifier {
    fn held(self, flags: CGEventFlags) -> bool {
        let mask = match self {
            Self::Command => CGEventFlags::MaskCommand,
            Self::Option => CGEventFlags::MaskAlternate,
        };
        flags.contains(mask)
    }
}

struct Candidate {
    pid: i32,
    wid: u32,
    app: String,
    title: String,
}

struct Live {
    candidates: Vec<Candidate>,
    selection: model::Session,
    trigger: Modifier,
}

/// Shared behind an `Rc<RefCell>` between the hot-key and tap callbacks. Both
/// run to completion on the one main run loop, so the borrows never overlap —
/// this holds only while no method here spins a nested run loop (e.g. a modal).
struct App {
    ws: Rc<winsrv::WindowServer>,
    strip: ui::Strip,
    session: Option<Live>,
    /// The in-session key tap, enabled only while a session is open.
    keys: Option<input::TapHandle>,
    /// Last modifier flags seen by the flags tap, kept current even between
    /// sessions so `trigger` can tell whether the trigger key is still down.
    held: CGEventFlags,
}

impl App {
    /// A trigger hot key fired. Opens a session on the first press; once open,
    /// cycling is driven by the key tap, so re-fires are ignored.
    fn trigger(&mut self, id: u32) {
        if self.session.is_some() {
            return;
        }
        let modifier = if id <= 2 {
            Modifier::Command
        } else {
            Modifier::Option
        };
        let backward = id == 2 || id == 4;

        let candidates = self.enumerate(modifier);
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            return;
        };
        if backward {
            selection.select(candidates.len() - 1);
        }
        self.session = Some(Live {
            candidates,
            selection,
            trigger: modifier,
        });

        // Hot-key dispatch lags the flags tap, so the trigger modifier may
        // already be up by now — a flick too quick to leave a release event.
        // Jump straight away without ever showing the strip in that case.
        if modifier.held(self.held) {
            self.set_keys_enabled(true);
            self.render();
        } else {
            self.jump();
        }
    }

    /// A key was pressed while a session is open: Tab (and its auto-repeats)
    /// moves the selection, Escape cancels; both are swallowed so the focused
    /// app never sees them. Everything else passes through.
    fn on_key(&mut self, event: &objc2_core_graphics::CGEvent) -> input::Disposition {
        if self.session.is_none() {
            return input::Disposition::Keep;
        }
        let code = input::keycode(event);
        if code == i64::from(input::KEY_TAB) {
            let backward = input::flags(event).contains(CGEventFlags::MaskShift);
            if let Some(live) = &mut self.session {
                live.selection.cycle(backward);
                self.strip.select(live.selection.selected());
            }
            input::Disposition::Swallow
        } else if code == i64::from(input::KEY_ESCAPE) {
            self.session = None;
            self.strip.hide();
            self.set_keys_enabled(false);
            input::Disposition::Swallow
        } else {
            input::Disposition::Keep
        }
    }

    /// Modifier flags changed: track the live state, and if a session is open
    /// and its trigger modifier is no longer held, jump to the selection.
    fn flags_changed(&mut self, flags: CGEventFlags) {
        self.held = flags;
        let Some(live) = &self.session else { return };
        if !live.trigger.held(flags) {
            self.jump();
        }
    }

    /// Ends the session and focuses the current selection. The focus is
    /// deferred to the next run-loop turn so it lands outside the event handler
    /// that triggered it — the `WindowServer` ignores it otherwise.
    fn jump(&mut self) {
        let Some(live) = self.session.take() else {
            return;
        };
        self.strip.hide();
        self.set_keys_enabled(false);
        if let Some(target) = live.candidates.get(live.selection.selected()) {
            let ws = self.ws.clone();
            let (pid, wid) = (target.pid, target.wid);
            on_next_turn(move || {
                ws.focus_window(pid, wid);
            });
        }
    }

    fn set_keys_enabled(&self, enabled: bool) {
        if let Some(keys) = &self.keys {
            keys.set_enabled(enabled);
        }
    }

    fn enumerate(&self, modifier: Modifier) -> Vec<Candidate> {
        let space_ids: Vec<u64> = self.ws.spaces().iter().map(|s| s.id).collect();
        let mut windows = self.ws.windows(&space_ids);
        windows.retain(|w| w.level == 0);
        if modifier == Modifier::Option
            && let Some(front) = windows.first().map(|w| w.pid)
        {
            windows.retain(|w| w.pid == front);
        }
        windows
            .into_iter()
            .map(|w| Candidate {
                pid: w.pid,
                wid: w.wid,
                app: w.app.unwrap_or_default(),
                title: w.title.unwrap_or_default(),
            })
            .collect()
    }

    fn render(&self) {
        let Some(live) = &self.session else { return };
        let tiles: Vec<ui::Tile> = live
            .candidates
            .iter()
            .map(|c| ui::Tile {
                app: c.app.clone(),
                title: c.title.clone(),
            })
            .collect();
        self.strip.show(&tiles, live.selection.selected());
    }
}

/// Boots the resident app: suppresses the native switcher, binds the triggers
/// and the release tap, then runs the main loop until quit.
pub fn run(mtm: MainThreadMarker) {
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let ws = match winsrv::WindowServer::connect() {
        Ok(ws) => ws,
        Err(missing) => {
            println!("skylight: unavailable — {}", missing.join(", "));
            return;
        }
    };
    let strip = ui::Strip::new(mtm);
    let app = Rc::new(RefCell::new(App {
        ws: Rc::new(ws),
        strip,
        session: None,
        keys: None,
        held: CGEventFlags::empty(),
    }));

    let triggers = [
        (1, input::CMD),
        (2, input::CMD | input::SHIFT),
        (3, input::OPTION),
        (4, input::OPTION | input::SHIFT),
    ]
    .map(|(id, modifiers)| input::Trigger {
        id,
        key: input::KEY_TAB,
        modifiers,
    });

    let on_trigger = app.clone();
    let Some(_hotkeys) = input::Hotkeys::register(&triggers, move |id| {
        on_trigger.borrow_mut().trigger(id);
    }) else {
        println!("input: could not register triggers");
        return;
    };

    let on_key = app.clone();
    let key_mask = input::event_mask(&[CGEventType::KeyDown]);
    let Some(key_tap) =
        input::EventTap::install(CGEventTapOptions::Default, key_mask, move |_ty, ev| {
            on_key.borrow_mut().on_key(ev)
        })
    else {
        println!("input: could not install the session key tap (accessibility?)");
        return;
    };
    let keys = key_tap.handle();
    keys.set_enabled(false);
    app.borrow_mut().keys = Some(keys);

    let on_flags = app.clone();
    let mask = input::event_mask(&[CGEventType::FlagsChanged]);
    let Some(_tap) =
        input::EventTap::install(CGEventTapOptions::ListenOnly, mask, move |_ty, ev| {
            on_flags.borrow_mut().flags_changed(input::flags(ev));
            input::Disposition::Keep
        })
    else {
        println!("input: could not install the release tap (accessibility?)");
        return;
    };

    let suppression = input::Suppression::engage();
    if suppression.is_none() {
        println!("input: native switcher not suppressed — it stays live alongside Oriel");
    }
    ns_app.run();
}
