//! The M1 loop: hold a trigger, cycle with taps, release to jump.

use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak};

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGEventFlags, CGEventTapOptions, CGEventType, CGImage};

/// Runs `work` on the main run loop after `delay_ms`, outside the current event
/// handler. Focus can't be done from inside the hot-key handler that consumed
/// the triggering event (the `WindowServer` ignores it), and the `WindowServer`
/// also needs a moment to settle after the event before it accepts one.
/// Callers capture `Rc` and must already be on the main thread.
fn on_main_after(delay_ms: u64, work: impl FnOnce() + 'static) {
    let mtm = MainThreadMarker::new().expect("on_main_after requires the main thread");
    let work = MainThreadBound::new(work, mtm);
    let nanos = i64::try_from(delay_ms)
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000_000);
    let when = DispatchTime::NOW.time(nanos);
    let _ = DispatchQueue::main().after(when, move || {
        let mtm = MainThreadMarker::new().expect("dispatch main queue");
        let work = work.into_inner(mtm);
        let _ = std::panic::catch_unwind(AssertUnwindSafe(work));
    });
}

/// Runs `work` on the main run loop; callable from any thread — the capture
/// worker uses it to hand finished previews back.
fn on_main(work: impl FnOnce() + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(work));
    });
}

thread_local! {
    /// The live `App`, reachable from main-thread callbacks that originate on
    /// another thread and so cannot capture the `Rc` directly.
    static APP: RefCell<Weak<RefCell<App>>> = const { RefCell::new(Weak::new()) };
}

/// Sets up the capture pipeline, when Screen Recording permission allows one:
/// results come back on the worker thread, hop to the main loop, and land in
/// `preview_ready`. Without permission there is no worker and the strip stays
/// on the icon layout (said once, in main).
fn spawn_capture(app: &Rc<RefCell<App>>) {
    if !capture::permitted() {
        return;
    }
    let Some(capturer) = capture::Capturer::new() else {
        println!("capture: unavailable — previews disabled");
        return;
    };
    let worker = capture::Worker::spawn(capturer, |wid, image| {
        on_main(move || {
            if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                app.borrow_mut().preview_ready(wid, image);
            }
        });
    });
    app.borrow_mut().worker = Some(worker);
    app.borrow().warm();
}

/// Focuses `wid`, retrying on failure — the `WindowServer` often rejects the
/// first activation after an event and accepts a slightly later one. Bails if a
/// newer jump has bumped `generation`, so a stale retry can't front a window
/// the user has already switched away from.
fn focus_with_retry(f: &Focus, pid: i32, wid: u32, attempts_left: u32) {
    if f.generation.get() != f.stamp {
        return;
    }
    f.ws.focus_window(pid, wid);
    if ax::raise_window(pid, wid) || attempts_left == 0 {
        return;
    }
    let f = f.clone();
    on_main_after(40, move || {
        focus_with_retry(&f, pid, wid, attempts_left - 1);
    });
}

/// What a retry chain needs: the `WindowServer`, plus a generation stamp to
/// tell whether this jump is still the current one.
#[derive(Clone)]
struct Focus {
    ws: Rc<winsrv::WindowServer>,
    generation: Rc<Cell<u32>>,
    stamp: u32,
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
    badge: String,
    aspect: f64,
}

fn tile_of(c: &Candidate, preview: Option<CFRetained<CGImage>>) -> ui::Tile {
    ui::Tile {
        app: c.app.clone(),
        title: c.title.clone(),
        pid: c.pid,
        aspect: c.aspect,
        preview,
        badge: c.badge.clone(),
        ..ui::Tile::default()
    }
}

/// A pleasant tile shape for windows whose frame the `WindowServer` won't say.
const FALLBACK_ASPECT: f64 = 1.6;

fn aspect_of(w: &winsrv::WindowInfo) -> f64 {
    if w.width > 0.0 && w.height > 0.0 {
        w.width / w.height
    } else {
        FALLBACK_ASPECT
    }
}

fn switchable(w: &winsrv::WindowInfo) -> bool {
    w.level == 0 && model::WindowState::decode(w.tags, w.attributes).switchable()
}

/// The preview cache's hard byte budget; at tile-sized thumbnails this is on
/// the order of a hundred windows.
const PREVIEW_BUDGET: usize = 32 << 20;

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
    /// Bumped on every jump; a retry chain stops once it no longer matches.
    generation: Rc<Cell<u32>>,
    /// Our own most-recently-focused order, since the `WindowServer` stacking
    /// query is unreliable for it.
    mru: model::Mru,
    /// Tile-sized previews by window id; the strip always paints from here and
    /// never waits on a capture.
    cache: capture::Cache<CFRetained<CGImage>>,
    /// `None` without Screen Recording permission — the strip then stays on
    /// the icon layout.
    worker: Option<capture::Worker>,
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

    /// Ends the session and focuses the current selection, deferred briefly so
    /// the focus lands after the triggering event settles (see `on_main_after`).
    fn jump(&mut self) {
        let Some(live) = self.session.take() else {
            return;
        };
        self.strip.hide();
        self.set_keys_enabled(false);
        let stamp = self.generation.get().wrapping_add(1);
        self.generation.set(stamp);
        if let Some(target) = live.candidates.get(live.selection.selected()) {
            let focus = Focus {
                ws: self.ws.clone(),
                generation: self.generation.clone(),
                stamp,
            };
            let (pid, wid) = (target.pid, target.wid);
            self.mru.touch(model::WindowId(wid));
            on_main_after(20, move || focus_with_retry(&focus, pid, wid, 5));
        }
    }

    fn set_keys_enabled(&self, enabled: bool) {
        if let Some(keys) = &self.keys {
            keys.set_enabled(enabled);
        }
    }

    fn enumerate(&mut self, modifier: Modifier) -> Vec<Candidate> {
        let spaces = self.ws.spaces();
        let map = model::SpaceMap::new(spaces.iter().map(|s| model::SpaceDesc {
            id: s.id,
            current: s.current,
            fullscreen: s.fullscreen,
        }));
        let space_ids: Vec<u64> = spaces.iter().map(|s| s.id).collect();
        let mut windows = self.ws.windows(&space_ids);
        windows.retain(switchable);

        // Order by our own recency, not the WindowServer stacking query, which
        // stops reflecting focus after a couple of switches.
        let ids: Vec<model::WindowId> = windows.iter().map(|w| model::WindowId(w.wid)).collect();
        self.mru.sync(&ids);
        self.cache.retain(|wid| ids.contains(&model::WindowId(wid)));
        let mut by_wid: std::collections::HashMap<u32, winsrv::WindowInfo> =
            windows.into_iter().map(|w| (w.wid, w)).collect();
        let mut ordered: Vec<winsrv::WindowInfo> = self
            .mru
            .order()
            .iter()
            .filter_map(|id| by_wid.remove(&id.0))
            .collect();

        if modifier == Modifier::Option
            && let Some(front) = ordered.first().map(|w| w.pid)
        {
            ordered.retain(|w| w.pid == front);
        }
        ordered
            .into_iter()
            .map(|w| {
                let state = model::WindowState::decode(w.tags, w.attributes);
                // The per-window Space lookup is an IPC round-trip; skip it
                // when every Space is a current user desktop (no badge possible).
                let space = if map.uniform() {
                    None
                } else {
                    self.ws.window_space(w.wid)
                };
                Candidate {
                    pid: w.pid,
                    wid: w.wid,
                    badge: map.badge(state.minimized, space),
                    aspect: aspect_of(&w),
                    app: w.app.unwrap_or_default(),
                    title: w.title.unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Paints the strip for the current session: previews straight from cache
    /// (stale beats late — first paint never waits on a capture), then a
    /// refresh of every visible window queued behind it.
    fn render(&mut self) {
        let Some(live) = &self.session else { return };
        let cache = &mut self.cache;
        let tiles: Vec<ui::Tile> = live
            .candidates
            .iter()
            .map(|c| tile_of(c, cache.shown(c.wid).cloned()))
            .collect();
        self.strip.show(&tiles, live.selection.selected());
        for c in &live.candidates {
            self.request_preview(c.wid);
        }
    }

    /// A fresh capture arrived from the worker: cache it, and if that window
    /// is on screen right now, repaint its tile.
    fn preview_ready(&mut self, wid: u32, image: CFRetained<CGImage>) {
        let bytes = capture::cost(&image);
        self.cache.insert(wid, image.clone(), bytes);
        let Some(live) = &self.session else { return };
        let Some(index) = live.candidates.iter().position(|c| c.wid == wid) else {
            return;
        };
        self.strip
            .update_tile(index, &tile_of(&live.candidates[index], Some(image)));
    }

    /// Pre-captures every switchable window, so the first summon paints
    /// previews instead of icons. Deliberately lighter than `enumerate`: just
    /// the window list, no MRU, Space, or badge work.
    fn warm(&self) {
        if self.worker.is_none() {
            return;
        }
        let space_ids: Vec<u64> = self.ws.spaces().iter().map(|s| s.id).collect();
        for w in self.ws.windows(&space_ids) {
            if switchable(&w) {
                self.request_preview(w.wid);
            }
        }
    }

    fn request_preview(&self, wid: u32) {
        if let Some(worker) = &self.worker {
            worker.request(wid);
        }
    }
}

/// The window id of the current frontmost app's focused window, so the MRU can
/// track switches the user makes outside Oriel (a click, an app's shortcut).
fn front_window() -> Option<u32> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    ax::focused_window(app.processIdentifier())
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
        generation: Rc::new(Cell::new(0)),
        mru: model::Mru::default(),
        cache: capture::Cache::new(PREVIEW_BUDGET),
        worker: None,
    }));
    APP.with(|slot| *slot.borrow_mut() = Rc::downgrade(&app));
    spawn_capture(&app);

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

    // Track focus changes made outside Oriel: on each app activation, record
    // where the user landed so the MRU order stays honest, and refresh that
    // window's preview while it is front and current.
    let on_activate = app.clone();
    let block = block2::RcBlock::new(
        move |_: core::ptr::NonNull<objc2_foundation::NSNotification>| {
            if let Some(wid) = front_window() {
                let mut app = on_activate.borrow_mut();
                app.mru.touch(model::WindowId(wid));
                app.request_preview(wid);
            }
        },
    );
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let _observer = unsafe {
        workspace
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(objc2_app_kit::NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &block,
            )
    };

    let suppression = input::Suppression::engage();
    if suppression.is_none() {
        println!("input: native switcher not suppressed — it stays live alongside Oriel");
    }
    ns_app.run();
}
