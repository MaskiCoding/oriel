//! The M1 loop: hold a trigger, cycle with taps, release to jump.

#[path = "keys.rs"]
mod keys;
#[path = "reload.rs"]
mod reload;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::mpsc::{Sender, channel};

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication, NSScreen};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapOptions, CGEventType,
    CGImage, CGWarpMouseCursorPosition,
};

use crate::lens::{self, Binding};
use crate::login;
use crate::menubar::{MenuBar, MenuCommand};
use keys::{ActionKeys, Dir, KEY_DELETE};

/// Runs `work` on the main run loop after `delay_ms`, outside the current event
/// handler. Focus can't be done from inside the hot-key handler that consumed
/// the triggering event (the `WindowServer` ignores it), and the `WindowServer`
/// also needs a moment to settle after the event before it accepts one.
/// Callers capture `Rc` and must already be on the main thread.
/// How often the process table is swept. Each sweep measured at 1-2 ms, so a
/// steady beat at the sample window is far below anything a user could feel.
/// It is the sample window itself: the threshold is calibrated against that
/// span, so a tick of any other length would silently change what counts as
/// working.
const LANTERN_TICK_MS: u64 =
    crate::lantern::WINDOW.as_secs() * 1000 + crate::lantern::WINDOW.subsec_millis() as u64;

/// Re-arms itself for the life of the process.
fn schedule_lantern() {
    on_main_after(LANTERN_TICK_MS, || {
        let Some(app) = APP.with(|slot| slot.borrow().upgrade()) else {
            return;
        };
        // Sample and read the counts under one borrow, then drop it before
        // touching the status item: rebuilding that re-enters this borrow.
        let (count, live) = {
            let mut app = app.borrow_mut();
            if app.paused || !app.config.lantern.enabled {
                // Not merely skipped: the samples either side of a pause are not
                // a measurement of anything.
                app.lantern.reset();
                (0, false)
            } else {
                app.lantern.poll();
                (app.lantern.count(), app.session.is_some())
            }
        };
        if let Some(menubar) = app.borrow().menubar.as_ref() {
            menubar.set_working(count);
        }
        if live {
            app.borrow_mut().relight();
        }
        schedule_lantern();
    });
}

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
    let Some(worker) = capture::Worker::spawn(capturer, |wid, image| {
        on_main(move || {
            if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                app.borrow_mut().preview_ready(wid, image);
            }
        });
    }) else {
        println!("capture: could not start the preview worker — previews disabled");
        return;
    };
    app.borrow_mut().worker = Some(worker);
    app.borrow().warm();
}

/// Full-resolution Peek captures on a dedicated thread. Coalesces to the latest
/// `(wid, stamp)` so a fast Tab-hold never queues one capture per keystroke.
/// Results hop back via `on_main` — never enter the tile cache.
fn spawn_peek_capture(app: &Rc<RefCell<App>>) {
    if !capture::permitted() {
        return;
    }
    let Some(capturer) = capture::Capturer::new() else {
        return;
    };
    let (tx, rx) = channel::<(u32, u32)>();
    let thread = std::thread::Builder::new()
        .name("peek".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut latest = first;
                while let Ok(next) = rx.try_recv() {
                    latest = next;
                }
                let (wid, stamp) = latest;
                let Some(image) = capturer.window_image(wid) else {
                    continue;
                };
                on_main(move || {
                    if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                        app.borrow().peek_ready(wid, stamp, &image);
                    }
                });
            }
        })
        .ok();
    if thread.is_some() {
        app.borrow_mut().peek_tx = Some(tx);
    }
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
    if ax::raise_window(pid, wid) {
        return;
    }
    // A per-window raise is refused outright while the front Space belongs to a
    // fullscreen app: the WindowServer will not leave that Space for one window.
    // AppKit activation does leave it — measured — so ask for that and let the
    // retry put the intended window on top once we are back on a desktop.
    // Without this the strip appears over a fullscreen app, takes the pick, and
    // strands the user exactly where they started.
    activate_pid(pid);
    if attempts_left == 0 {
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

#[derive(Clone)]
struct Candidate {
    pid: i32,
    wid: u32,
    app: String,
    title: String,
    badge: String,
    aspect: f64,
    title_spans: Vec<(usize, usize)>,
    app_spans: Vec<(usize, usize)>,
}

fn tile_of(c: &Candidate, preview: Option<CFRetained<CGImage>>, lantern: bool) -> ui::Tile {
    ui::Tile {
        lantern,
        app: c.app.clone(),
        title: c.title.clone(),
        pid: c.pid,
        aspect: c.aspect,
        preview,
        badge: c.badge.clone(),
        title_spans: c.title_spans.clone(),
        app_spans: c.app_spans.clone(),
    }
}

fn switchable(w: &winsrv::WindowInfo) -> bool {
    w.level == 0
        && model::WindowState::decode(w.tags, w.attributes).switchable()
        && model::WindowState::meets_min_size(w.width, w.height)
}

/// After a jump's focus sequence has had time to settle, optionally warp the
/// pointer to the focused window's centre.
fn warp_cursor_to_window(ws: &winsrv::WindowServer, wid: u32, mode: config::CursorFollowsFocus) {
    use config::CursorFollowsFocus::{Never, OtherScreen};

    if matches!(mode, Never) || crate::snapshot::is_windowless(wid) {
        return;
    }
    let space_ids: Vec<u64> = ws.spaces().iter().map(|s| s.id).collect();
    let Some(w) = ws.windows(&space_ids).into_iter().find(|w| w.wid == wid) else {
        return;
    };
    let frames = crate::snapshot::screen_frames();
    if matches!(mode, OtherScreen) {
        let Some(mouse) = cursor_location() else {
            return;
        };
        let mouse_screen = winsrv::screen_index((mouse.x, mouse.y, 0.0, 0.0), &frames);
        let win_screen = winsrv::screen_index((w.x, w.y, w.width, w.height), &frames);
        if mouse_screen == win_screen {
            return;
        }
    }
    let point = CGPoint::new(w.x + w.width / 2.0, w.y + w.height / 2.0);
    let _ = CGWarpMouseCursorPosition(point);
}

/// Fronts an app that has no open window, so selecting it still does the
/// obvious thing. The window focus sequence cannot help here — there is no
/// window id to raise.
fn activate_pid(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::ActivateAllWindows);
    }
}

fn cursor_location() -> Option<CGPoint> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)?;
    let event = CGEvent::new(Some(&source))?;
    Some(CGEvent::location(Some(&event)))
}

fn as_window(c: &Candidate) -> model::Window {
    model::Window {
        id: model::WindowId(c.wid),
        app: c.pid,
        app_name: c.app.clone(),
        title: c.title.clone(),
        state: model::WindowState::decode(0, 0x2),
        fullscreen: false,
        space: None,
        space_visible: true,
        space_ordinal: 0,
        screen: 0,
        created: 0,
        is_main: true,
        windowless: false,
    }
}

fn apply_spans(c: &mut Candidate, m: Option<&model::Match>) {
    c.title_spans.clear();
    c.app_spans.clear();
    let Some(m) = m else {
        return;
    };
    match m.field {
        model::Field::Title => c.title_spans.clone_from(&m.spans),
        model::Field::App => c.app_spans.clone_from(&m.spans),
    }
}

fn rank_filter(base: Vec<Candidate>, query: &str) -> Vec<Candidate> {
    let windows: Vec<model::Window> = base.iter().map(as_window).collect();
    let ordered = model::filter(query, &windows);
    let mut by_id: std::collections::HashMap<u32, Candidate> =
        base.into_iter().map(|c| (c.wid, c)).collect();
    let mut candidates = Vec::with_capacity(ordered.len());
    for (id, m) in ordered {
        let Some(mut c) = by_id.remove(&id.0) else {
            continue;
        };
        apply_spans(&mut c, m.as_ref());
        candidates.push(c);
    }
    candidates
}

fn quit_pid(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        let _ = app.terminate();
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/oriel/config.toml")
}

/// The preview cache's hard byte budget; at tile-sized thumbnails this is on
/// the order of a hundred windows.
const PREVIEW_BUDGET: usize = 32 << 20;

struct Live {
    /// The unfiltered candidates this session was built from. Filter ranks
    /// from these so typing does not re-enumerate the `WindowServer` per key.
    base: Vec<Candidate>,
    candidates: Vec<Candidate>,
    selection: model::Session,
    binding_idx: usize,
    /// Whether the strip has been shown (apparition delay may still be pending).
    shown: bool,
    /// Epoch captured when this session's reveal was scheduled; stale on cancel/jump.
    show_epoch: u32,
    /// `None` = not filtering. `Some` = Filter mode with that query (may be empty).
    filter: Option<String>,
}

/// Shared behind an `Rc<RefCell>` between the hot-key and tap callbacks. Both
/// run to completion on the one main run loop, so the borrows never overlap —
/// this holds only while no method here spins a nested run loop (e.g. a modal).
struct App {
    mtm: MainThreadMarker,
    ws: Rc<winsrv::WindowServer>,
    strip: ui::Strip,
    peek: ui::Peek,
    /// Latest Peek request stamp; in-flight captures with an older stamp are dropped.
    peek_stamp: Cell<u32>,
    /// Sends `(wid, stamp)` to the Peek capture thread. `None` without permission.
    peek_tx: Option<Sender<(u32, u32)>>,
    session: Option<Live>,
    bindings: Vec<Binding>,
    summon_delay_ms: u32,
    /// When the current trigger fired, for the first-paint measurement.
    #[cfg(debug_assertions)]
    triggered_at: Option<std::time::Instant>,
    /// Bumped to cancel a pending apparition-delay reveal.
    show_epoch: u32,
    /// The in-session key tap, enabled only while a session is open.
    keys: Option<input::TapHandle>,
    action_keys: ActionKeys,
    controls: config::Controls,
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
    config: config::Config,
    config_path: PathBuf,
    rules: model::Rules,
    /// Bundle-ID cache keyed by pid — lookups are on the summon hot path.
    bundles: HashMap<i32, Option<String>>,
    /// Carbon hot keys. Dropped (unregistered) while paused or while the
    /// frontmost app's rule says pass-through — Carbon consumes the combo once
    /// registered, so the only way to let the focused app see it is to not hold
    /// the registration.
    hotkeys: Option<input::Hotkeys>,
    suppression: Option<input::Suppression>,
    paused: bool,
    menubar: Option<MenuBar>,
    lantern: crate::lantern::Lantern,
    /// Which tiles were painted lit, so a poll only repaints what changed.
    lit: Vec<bool>,
    settings: Option<ui::Settings>,
    /// Config arrived while a session was open; applied on close.
    pending_config: Option<config::Config>,
}

impl App {
    fn binding(&self, idx: usize) -> Option<&Binding> {
        self.bindings.get(idx)
    }

    /// A trigger hot key fired. Opens a session on the first press; once open,
    /// the same lens is ignored (cycling is the key tap's job) and a different
    /// lens morphs the strip in place.
    fn trigger(&mut self, id: u32) {
        if self.paused {
            return;
        }
        // An app launched since the last summon should be attributable now.
        self.lantern.refresh_apps();
        #[cfg(debug_assertions)]
        {
            self.triggered_at = Some(std::time::Instant::now());
        }
        let (idx, backward) = lens::decode_hotkey_id(id);
        if idx >= self.bindings.len() {
            return;
        }

        if let Some(live) = &self.session {
            if live.binding_idx == idx {
                return;
            }
            self.morph(idx);
            return;
        }

        let Some(binding) = self.binding(idx).cloned() else {
            return;
        };
        let candidates = self.enumerate(&binding, true);
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            self.strip.end_session();
            return;
        };
        if backward {
            selection.select(candidates.len() - 1);
        }

        let epoch = self.show_epoch;
        // Armed with the session, not with the reveal: during an apparition
        // delay the strip is still invisible but Tab and Escape must already work.
        self.set_keys_enabled(true);
        self.session = Some(Live {
            base: candidates.clone(),
            candidates,
            selection,
            binding_idx: idx,
            shown: false,
            show_epoch: epoch,
            filter: None,
        });
        self.strip.set_look(self.degraded(binding.look));

        let linger = lens::stays_open(binding.on_release);
        let held = binding.hold.is_empty() || self.held.contains(binding.hold);

        // Hot-key dispatch lags the flags tap, so the trigger modifier may
        // already be up by now — a flick too quick to leave a release event.
        // Jump straight away without ever showing the strip in that case.
        if !linger && !held {
            self.jump();
            return;
        }
        // Same race, other branch: a Filter lens released this fast would open
        // a session and then never see a release event to enter Filter on.
        if binding.on_release == config::OnRelease::Filter && !held {
            self.enter_filter(None);
            return;
        }

        if self.summon_delay_ms == 0 {
            self.reveal();
        } else {
            let delay = u64::from(self.summon_delay_ms);
            on_main_after(delay, move || {
                if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                    app.borrow_mut().reveal_if(epoch);
                }
            });
        }
    }

    /// Mid-session lens change: re-resolve, apply look, keep the panel up.
    fn morph(&mut self, idx: usize) {
        let Some(binding) = self.binding(idx).cloned() else {
            return;
        };
        let previous_show_on = self
            .session
            .as_ref()
            .and_then(|l| self.binding(l.binding_idx))
            .map(|b| b.look.show_on);
        let candidates = self.enumerate(&binding, previous_show_on != Some(binding.look.show_on));
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            self.cancel();
            return;
        };
        selection.select(0);
        let shown = self.session.as_ref().is_some_and(|l| l.shown);
        let show_epoch = self
            .session
            .as_ref()
            .map_or(self.show_epoch, |l| l.show_epoch);
        self.strip.set_query(None);
        self.session = Some(Live {
            base: candidates.clone(),
            candidates,
            selection,
            binding_idx: idx,
            shown,
            show_epoch,
            filter: None,
        });
        self.strip.set_look(self.degraded(binding.look));
        if shown {
            self.render();
        }
    }

    /// Without Screen Recording there are no previews, so Gallery would be a
    /// grid of fallback icons in preview-shaped boxes. Show Icons instead —
    /// PRD §4.8 and §8.6 make the degraded mode first-class, not an accident.
    fn degraded(&self, mut look: ui::Look) -> ui::Look {
        if self.worker.is_none() && look.style == ui::Style::Gallery {
            look.style = ui::Style::Icons;
        }
        look
    }

    fn reveal_if(&mut self, epoch: u32) {
        let Some(live) = &self.session else {
            return;
        };
        if live.shown || live.show_epoch != epoch || self.show_epoch != epoch {
            return;
        }
        self.reveal();
    }

    fn reveal(&mut self) {
        let Some(live) = &mut self.session else {
            return;
        };
        if live.shown {
            return;
        }
        live.shown = true;
        self.render();
        // PRD §8.2: first paint within ~33 ms of the trigger, warm cache.
        // Measured from the hot key, so it covers enumeration and resolve too.
        #[cfg(debug_assertions)]
        if let Some(at) = self.triggered_at.take()
            && std::env::var_os("ORIEL_TIME_PAINT").is_some()
        {
            println!("first-paint {:.1} ms", at.elapsed().as_secs_f64() * 1000.0);
        }
    }

    /// A key was pressed while a session is open. Bound actions and Filter
    /// input are swallowed; everything else passes through.
    fn on_key(&mut self, event: &objc2_core_graphics::CGEvent) -> input::Disposition {
        if self.session.is_none() {
            return input::Disposition::Keep;
        }
        let code = input::keycode(event);
        let flags = input::flags(event);
        let in_filter = self.session.as_ref().is_some_and(|l| l.filter.is_some());

        if code == i64::from(input::KEY_TAB) {
            let backward = flags.contains(CGEventFlags::MaskShift);
            if let Some(live) = &mut self.session {
                live.selection.cycle(backward);
                self.strip.select(live.selection.selected());
            }
            self.request_peek();
            return input::Disposition::Swallow;
        }

        if self.action_keys.cancel == Some(code) {
            if in_filter {
                self.exit_filter();
            } else {
                self.cancel();
            }
            return input::Disposition::Swallow;
        }

        if self.action_keys.focus == Some(code) {
            self.jump();
            return input::Disposition::Swallow;
        }

        if in_filter {
            if code == KEY_DELETE {
                self.filter_backspace();
                return input::Disposition::Swallow;
            }
            if !keys::has_cmd_or_ctrl(flags) {
                let chars = keys::event_chars(event);
                if keys::is_typing(&chars) {
                    self.filter_type(&chars);
                    return input::Disposition::Swallow;
                }
            }
            // Arrows still move; vim letters are typing above.
            if self.controls.arrow_keys {
                let dir = match code {
                    123 => Some(Dir::Left),
                    124 => Some(Dir::Right),
                    125 => Some(Dir::Down),
                    126 => Some(Dir::Up),
                    _ => None,
                };
                if let Some(dir) = dir {
                    self.move_sel(dir);
                    return input::Disposition::Swallow;
                }
            }
            return input::Disposition::Keep;
        }

        if self.action_keys.close == Some(code) {
            self.act_close();
            return input::Disposition::Swallow;
        }
        if self.action_keys.minimize == Some(code) {
            self.act_minimize();
            return input::Disposition::Swallow;
        }
        if self.action_keys.fullscreen == Some(code) {
            self.act_fullscreen();
            return input::Disposition::Swallow;
        }
        if self.action_keys.quit_app == Some(code) {
            self.act_quit();
            return input::Disposition::Swallow;
        }
        if self.action_keys.hide_app == Some(code) {
            self.act_hide();
            return input::Disposition::Swallow;
        }

        if let Some(dir) = keys::movement(code, &self.controls) {
            self.move_sel(dir);
            return input::Disposition::Swallow;
        }

        // Linger / Filter-release lenses: typing enters Filter directly.
        if self.stays_open() && !keys::has_cmd_or_ctrl(flags) {
            let chars = keys::event_chars(event);
            if keys::is_typing(&chars) {
                self.enter_filter(Some(&chars));
                return input::Disposition::Swallow;
            }
        }

        input::Disposition::Keep
    }

    fn stays_open(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|l| self.binding(l.binding_idx))
            .is_some_and(|b| lens::stays_open(b.on_release))
    }

    /// A tile was clicked: select it and jump, exactly as Return would.
    fn click_tile(&mut self, index: usize) {
        if !self.select_index(index) {
            return;
        }
        self.jump();
    }

    /// Scroll steps the selection the way Tab and Shift-Tab do.
    fn scroll_selection(&mut self, delta: i32) {
        if self.session.is_none() || delta == 0 {
            return;
        }
        if let Some(live) = &mut self.session {
            live.selection.cycle(delta < 0);
            let next = live.selection.selected();
            self.strip.select(next);
        }
        self.request_peek();
    }

    /// Hover moves the selection without ever committing to a jump.
    fn hover_tile(&mut self, index: usize) {
        if self.select_index(index) {
            self.request_peek();
        }
    }

    /// Moves the selection to `index` when a session is open and it is in
    /// range. `false` means the event was stale and should be ignored.
    fn select_index(&mut self, index: usize) -> bool {
        let Some(live) = &mut self.session else {
            return false;
        };
        if index >= live.candidates.len() {
            return false;
        }
        live.selection.select(index);
        self.strip.select(index);
        true
    }

    fn move_sel(&mut self, dir: Dir) {
        let Some(live) = &self.session else {
            return;
        };
        let n = live.candidates.len();
        let selected = live.selection.selected();
        let rows = self.strip.rows();
        let next = keys::step_rows(selected, n, &rows, dir);
        if let Some(live) = &mut self.session {
            live.selection.select(next);
        }
        self.strip.select(next);
        self.request_peek();
    }

    fn enter_filter(&mut self, initial: Option<&str>) {
        let Some(live) = &self.session else {
            return;
        };
        let need_reveal = !live.shown;
        let q = initial.unwrap_or("").to_owned();
        if let Some(live) = &mut self.session {
            live.filter = Some(q.clone());
        }
        self.strip.set_query(Some(&q));
        if need_reveal {
            self.reveal();
        }
        self.apply_filter();
    }

    fn exit_filter(&mut self) {
        let Some(live) = &self.session else {
            return;
        };
        let binding_idx = live.binding_idx;
        let selected_wid = live
            .candidates
            .get(live.selection.selected())
            .map(|c| c.wid);
        let shown = live.shown;
        let show_epoch = live.show_epoch;
        self.strip.set_query(None);
        let Some(binding) = self.binding(binding_idx).cloned() else {
            return;
        };
        let candidates = self.enumerate(&binding, false);
        if candidates.is_empty() {
            self.cancel();
            return;
        }
        let mut selection = model::Session::start(candidates.len()).unwrap();
        if let Some(wid) = selected_wid
            && let Some(i) = candidates.iter().position(|c| c.wid == wid)
        {
            selection.select(i);
        }
        self.session = Some(Live {
            base: candidates.clone(),
            candidates,
            selection,
            binding_idx,
            shown,
            show_epoch,
            filter: None,
        });
        if shown {
            self.render();
        }
    }

    fn filter_type(&mut self, chars: &str) {
        let shown = {
            let Some(live) = &mut self.session else {
                return;
            };
            let Some(q) = &mut live.filter else {
                return;
            };
            q.push_str(chars);
            q.clone()
        };
        self.strip.set_query(Some(&shown));
        self.apply_filter();
    }

    fn filter_backspace(&mut self) {
        let shown = {
            let Some(live) = &mut self.session else {
                return;
            };
            let Some(q) = &mut live.filter else {
                return;
            };
            q.pop();
            q.clone()
        };
        self.strip.set_query(Some(&shown));
        self.apply_filter();
    }

    /// Reorder candidates by match quality; keep all tiles; select best (0).
    fn apply_filter(&mut self) {
        let Some(live) = &self.session else {
            return;
        };
        let Some(query) = live.filter.clone() else {
            return;
        };
        let binding_idx = live.binding_idx;
        let shown = live.shown;
        let show_epoch = live.show_epoch;
        let base = live.base.clone();
        if base.is_empty() {
            self.cancel();
            return;
        }
        let candidates = rank_filter(base.clone(), &query);
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            self.cancel();
            return;
        };
        selection.select(0);
        self.session = Some(Live {
            base,
            candidates,
            selection,
            binding_idx,
            shown,
            show_epoch,
            filter: Some(query),
        });
        if shown {
            self.render();
        }
    }

    fn selected_target(&self) -> Option<(i32, u32)> {
        let live = self.session.as_ref()?;
        let c = live.candidates.get(live.selection.selected())?;
        Some((c.pid, c.wid))
    }

    fn act_close(&mut self) {
        if let Some((pid, wid)) = self.selected_target() {
            // Only drop the row if the app accepted the close. A refusal — an
            // unsaved-changes sheet, a window that ignores the action — left the
            // strip claiming a window had gone while it sat there on screen.
            if ax::close_window(pid, wid) {
                self.forget(|c| c.wid != wid);
            }
        }
        self.settle();
    }

    fn act_minimize(&mut self) {
        // A failed read is unknown, not "not minimized" — toggling on a guess
        // would minimize a window the user asked to restore.
        if let Some((pid, wid)) = self.selected_target()
            && let Some(now) = ax::is_minimized(pid, wid)
        {
            let _ = ax::set_minimized(pid, wid, !now);
        }
        self.refresh_candidates();
    }

    fn act_fullscreen(&mut self) {
        // `None` means the window has no AXFullScreen at all, so there is
        // nothing to toggle — better than guessing "not fullscreen".
        if let Some((pid, wid)) = self.selected_target()
            && let Some(now) = ax::is_fullscreen(pid, wid)
        {
            let _ = ax::set_fullscreen(pid, wid, !now);
        }
        self.refresh_candidates();
    }

    fn act_quit(&mut self) {
        if let Some((pid, _)) = self.selected_target() {
            quit_pid(pid);
            // Every window of the app goes with it, not just the selected one.
            self.forget(|c| c.pid != pid);
        }
        self.settle();
    }

    fn act_hide(&mut self) {
        if let Some((pid, _)) = self.selected_target()
            && let Some(now) = ax::is_app_hidden(pid)
        {
            let _ = ax::set_app_hidden(pid, !now);
        }
        self.refresh_candidates();
    }

    /// Drops rows the user has just destroyed, before the system agrees.
    ///
    /// Quitting an app is a request, not an edit: the process takes time to go,
    /// so re-enumerating straight away lists windows that are already doomed and
    /// the strip keeps showing them. Removing them here means the strip never
    /// claims a window that is gone; [`Self::settle`] puts anything back that
    /// survived, so a refused quit corrects itself.
    fn forget(&mut self, keep: impl Fn(&Candidate) -> bool) {
        let Some(live) = &mut self.session else {
            return;
        };
        let before = live.candidates.len();
        live.candidates.retain(&keep);
        live.base.retain(&keep);
        if live.candidates.len() == before {
            return;
        }
        if live.candidates.is_empty() {
            // Do not cancel here. The rows were dropped on the user's say-so,
            // before the system agreed; cancelling would end the session and
            // take `settle` with it, so a refused quit could never be undone.
            // An empty strip for a moment is recoverable, a closed one is not.
            return;
        }
        let selected = live.selection.selected().min(live.candidates.len() - 1);
        let Some(mut selection) = model::Session::start(live.candidates.len()) else {
            return;
        };
        selection.select(selected);
        live.selection = selection;
        if live.shown {
            self.render();
        }
    }

    /// Re-enumerates once the system has had time to act on a destructive key.
    fn settle(&mut self) {
        let Some(epoch) = self.session.as_ref().map(|l| l.show_epoch) else {
            return;
        };
        on_main_after(450, move || {
            let Some(app) = APP.with(|slot| slot.borrow().upgrade()) else {
                return;
            };
            let mut app = app.borrow_mut();
            if app.session.as_ref().is_some_and(|l| l.show_epoch == epoch) {
                app.refresh_candidates();
            }
        });
    }

    /// Re-resolve after a destructive / state-changing action; clamp selection.
    fn refresh_candidates(&mut self) {
        let Some(live) = &self.session else {
            return;
        };
        let binding_idx = live.binding_idx;
        let selected = live.selection.selected();
        let filter = live.filter.clone();
        let shown = live.shown;
        let show_epoch = live.show_epoch;
        let Some(binding) = self.binding(binding_idx).cloned() else {
            return;
        };
        let base = self.enumerate(&binding, false);
        if base.is_empty() {
            self.cancel();
            return;
        }
        let candidates = match &filter {
            Some(query) => rank_filter(base.clone(), query),
            None => base.clone(),
        };
        if candidates.is_empty() {
            self.cancel();
            return;
        }
        let mut selection = model::Session::start(candidates.len()).unwrap();
        selection.select(selected.min(candidates.len() - 1));
        self.session = Some(Live {
            base,
            candidates,
            selection,
            binding_idx,
            shown,
            show_epoch,
            filter,
        });
        if shown {
            self.render();
        }
    }

    /// Modifier flags changed: track the live state, and if a session is open
    /// and its hold modifiers are no longer held, run the lens release action.
    fn flags_changed(&mut self, flags: CGEventFlags) {
        self.held = flags;
        let Some(live) = &self.session else {
            return;
        };
        let Some(binding) = self.binding(live.binding_idx) else {
            return;
        };
        if binding.hold.is_empty() {
            return;
        }
        if flags.contains(binding.hold) {
            return;
        }
        let on_release = binding.on_release;
        if lens::stays_open(on_release) {
            // Linger/Filter: strip stays; ensure it is visible if delay pending.
            if !live.shown {
                self.reveal();
            }
            if on_release == config::OnRelease::Filter
                && self.session.as_ref().is_some_and(|l| l.filter.is_none())
            {
                self.enter_filter(None);
            }
            return;
        }
        self.jump();
    }

    fn cancel(&mut self) {
        self.show_epoch = self.show_epoch.wrapping_add(1);
        self.peek_stamp.set(self.peek_stamp.get().wrapping_add(1));
        self.session = None;
        self.strip.set_query(None);
        self.strip.hide();
        self.strip.end_session();
        self.peek.hide();
        self.set_keys_enabled(false);
        self.flush_pending_config();
    }

    /// Ends the session and focuses the current selection, deferred briefly so
    /// the focus lands after the triggering event settles (see `on_main_after`).
    fn jump(&mut self) {
        self.show_epoch = self.show_epoch.wrapping_add(1);
        self.peek_stamp.set(self.peek_stamp.get().wrapping_add(1));
        let Some(live) = self.session.take() else {
            return;
        };
        self.strip.set_query(None);
        self.strip.hide();
        self.strip.end_session();
        self.peek.hide();
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
            if crate::snapshot::is_windowless(wid) {
                // No window to raise (PRD §4.1): just bring the app forward.
                on_main_after(20, move || activate_pid(pid));
            } else {
                on_main_after(20, move || focus_with_retry(&focus, pid, wid, 5));
            }
            // Warp after the focus retries have had time to settle — separate
            // from the focus chain so timing there is untouched.
            let mode = self.controls.cursor_follows_focus;
            if mode != config::CursorFollowsFocus::Never {
                let ws = self.ws.clone();
                // Same generation guard as the focus chain: a warp queued for a
                // jump the user has already moved on from must not fire.
                let generation = self.generation.clone();
                on_main_after(280, move || {
                    if generation.get() == stamp {
                        warp_cursor_to_window(&ws, wid, mode);
                    }
                });
            }
        }
        self.flush_pending_config();
    }

    fn set_keys_enabled(&self, enabled: bool) {
        if let Some(keys) = &self.keys {
            keys.set_enabled(enabled);
        }
    }

    /// Enumerates and, when `fix_screen` asks, pins the summon's screen from
    /// the same enumeration pass first, so `strip-screen` filtering and panel
    /// placement share one answer for the whole session.
    ///
    /// The active screen comes from the `WindowServer`'s z-order rather than
    /// the AX focused window: an AX round trip here would let a beach-balling
    /// frontmost app hold the summon for its ~1 s messaging timeout — the one
    /// moment a switcher must not wait on the app being escaped. Oriel's own
    /// MRU is not enough: it moves on app activation and on jumps, so a focus
    /// change *within* the frontmost app slips past it, and the strip would
    /// pin to the display of a window the user left. The z-order cannot be
    /// stale that way. The MRU-ordered snapshot stays as the fallback for the
    /// moments the ordered query answers with nothing.
    fn enumerate(&mut self, binding: &Binding, fix_screen: bool) -> Vec<Candidate> {
        let snap =
            crate::snapshot::snapshot_with(&self.ws, &mut self.mru, &self.rules, &mut self.bundles);
        let ids: Vec<model::WindowId> = snap.windows.iter().map(|w| w.id).collect();
        self.cache.retain(|wid| ids.contains(&model::WindowId(wid)));

        let active_app = frontmost_pid();
        if fix_screen {
            let active_screen = self.focused_window_screen().or_else(|| {
                snap.windows
                    .iter()
                    .find(|w| !w.windowless)
                    .map(|w| w.screen)
            });
            self.strip
                .begin_session(binding.look.show_on, active_screen);
        }

        let ctx = model::ResolveCtx {
            active_app,
            strip_screen: self.strip.screen_index(binding.look.show_on),
        };
        let ordered = model::resolve(&binding.model, &snap.windows, &ctx);

        let by_id: std::collections::HashMap<model::WindowId, &model::Window> =
            snap.windows.iter().map(|w| (w.id, w)).collect();
        ordered
            .into_iter()
            .filter_map(|id| {
                let w = by_id.get(&id)?;
                let meta = snap.meta.get(&id)?;
                Some(Candidate {
                    pid: meta.pid,
                    wid: id.0,
                    app: w.app_name.clone(),
                    title: w.title.clone(),
                    badge: meta.badge.clone(),
                    aspect: meta.aspect,
                    title_spans: Vec::new(),
                    app_spans: Vec::new(),
                })
            })
            .collect()
    }

    /// The screen holding the window with focus, read from the
    /// `WindowServer`'s z-order: the front-to-back query's first switchable
    /// row is the focused window, whoever owns it. `None` when there is no
    /// such window or no screen to place it on.
    fn focused_window_screen(&self) -> Option<u32> {
        let frames = crate::snapshot::screen_frames();
        if frames.is_empty() {
            return None;
        }
        let space_ids: Vec<u64> = self.ws.spaces().iter().map(|s| s.id).collect();
        self.ws
            .windows_ordered(&space_ids)
            .into_iter()
            .find(switchable)
            .map(|w| winsrv::screen_index((w.x, w.y, w.width, w.height), &frames))
    }

    /// Repaints only the tiles whose lit state changed since the last poll.
    /// A strip that is up while agents work must not rebuild itself every tick.
    fn relight(&mut self) {
        let Some(live) = self.session.as_ref() else {
            return;
        };
        if !live.shown {
            return;
        }
        let changed: Vec<(usize, Candidate, bool)> = live
            .candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    i,
                    c,
                    self.config.lantern.drift && self.lantern.working(model::Pid(c.pid)),
                )
            })
            .filter(|(i, _, want)| self.lit.get(*i).copied().unwrap_or(false) != *want)
            .map(|(i, c, want)| (i, c.clone(), want))
            .collect();
        for (index, _, want) in changed {
            // Fades the overlay in place instead of rebuilding the tile, so a
            // window that stops working ebbs rather than snapping, and the
            // preview underneath is never re-laid mid-session.
            self.strip.set_lantern(index, want);
            if let Some(slot) = self.lit.get_mut(index) {
                *slot = want;
            }
        }
    }

    /// Paints the strip for the current session: previews straight from cache
    /// (stale beats late — first paint never waits on a capture), then a
    /// refresh of every visible window queued behind it.
    fn render(&mut self) {
        let Some(live) = &self.session else {
            return;
        };
        let cache = &mut self.cache;
        let lantern = &self.lantern;
        let drift = self.config.lantern.drift;
        let tiles: Vec<ui::Tile> = live
            .candidates
            .iter()
            .map(|c| {
                tile_of(
                    c,
                    cache.shown(c.wid).cloned(),
                    drift && lantern.working(model::Pid(c.pid)),
                )
            })
            .collect();
        self.lit = tiles.iter().map(|t| t.lantern).collect();
        self.strip.show(&tiles, live.selection.selected());
        for c in &live.candidates {
            self.request_preview(c.wid);
        }
        self.request_peek();
    }

    /// A full-resolution Peek frame arrived. Shown only if the stamp still
    /// matches the current selection request and a session is live.
    fn peek_ready(&self, wid: u32, stamp: u32, image: &CGImage) {
        if stamp != self.peek_stamp.get() || !self.config.peek.enabled {
            return;
        }
        let Some(live) = &self.session else {
            return;
        };
        if !live.shown {
            return;
        }
        let Some(c) = live.candidates.get(live.selection.selected()) else {
            return;
        };
        if c.wid != wid {
            return;
        }
        let Some(screen) = self.peek_screen() else {
            return;
        };
        self.peek.show(image, &screen);
    }

    /// Asks the Peek worker for the current selection's full-res frame, or
    /// hides Peek when it should not be visible.
    fn request_peek(&self) {
        if !self.config.peek.enabled {
            self.peek.hide();
            return;
        }
        let Some(live) = &self.session else {
            self.peek.hide();
            return;
        };
        if !live.shown {
            return;
        }
        let Some(c) = live.candidates.get(live.selection.selected()) else {
            self.peek.hide();
            return;
        };
        if crate::snapshot::is_windowless(c.wid) {
            self.peek.hide();
            return;
        }
        let Some(tx) = &self.peek_tx else {
            return;
        };
        let stamp = self.peek_stamp.get().wrapping_add(1);
        self.peek_stamp.set(stamp);
        let _ = tx.send((c.wid, stamp));
    }

    fn peek_screen(&self) -> Option<Retained<NSScreen>> {
        let show_on = self
            .session
            .as_ref()
            .and_then(|l| self.binding(l.binding_idx))
            .map_or(ui::ShowOn::ActiveScreen, |b| b.look.show_on);
        let idx = usize::try_from(self.strip.screen_index(show_on)).unwrap_or(0);
        let screens = NSScreen::screens(self.mtm);
        if idx >= screens.count() {
            return screens.firstObject();
        }
        Some(screens.objectAtIndex(idx))
    }

    /// A fresh capture arrived from the worker: cache it, and if that window
    /// is on screen right now, repaint its tile.
    fn preview_ready(&mut self, wid: u32, image: CFRetained<CGImage>) {
        let bytes = capture::cost(&image);
        self.cache.insert(wid, image.clone(), bytes);
        let Some(live) = &self.session else {
            return;
        };
        let Some(index) = live.candidates.iter().position(|c| c.wid == wid) else {
            return;
        };
        let candidate = &live.candidates[index];
        let lit = self.config.lantern.drift && self.lantern.working(model::Pid(candidate.pid));
        self.strip
            .update_tile(index, &tile_of(candidate, Some(image), lit));
    }

    /// Pre-captures every switchable window, so the first summon paints
    /// previews instead of icons. Deliberately lighter than `enumerate`: just
    /// the window list, no MRU, Space, or badge work. Gated on
    /// `background_capture` — with it off, captures happen only during a session.
    fn warm(&self) {
        if !self.config.background_capture || self.worker.is_none() {
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
        // Synthetic windowless ids are not capturable.
        if crate::snapshot::is_windowless(wid) {
            return;
        }
        if let Some(worker) = &self.worker {
            worker.request(wid);
        }
    }

    fn frontmost_should_pass(&mut self) -> bool {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else {
            return false;
        };
        let pid = app.processIdentifier();
        let Some(bid) = crate::snapshot::bundle_id(pid, &mut self.bundles) else {
            return false;
        };
        let bid = bid.to_owned();
        // Only a `fullscreen` rule needs the AX round trip, and this runs on
        // every app activation — skip it for the common no-rule case.
        match self.rules.pass_mode(&bid) {
            model::PassTriggers::Never => false,
            model::PassTriggers::Always => true,
            model::PassTriggers::Fullscreen => ax::focused_window(pid)
                .and_then(|wid| ax::is_fullscreen(pid, wid))
                .unwrap_or(false),
        }
    }

    /// Register or drop Carbon hot keys so a pass-through frontmost app (or a
    /// paused Oriel) actually receives the trigger combo.
    fn sync_hotkeys(&mut self) {
        let pass = self.paused || self.frontmost_should_pass();
        if pass {
            self.hotkeys = None;
            return;
        }
        if self.hotkeys.is_some() {
            return;
        }
        let triggers = lens::triggers_for(&self.bindings);
        if triggers.is_empty() {
            return;
        }
        self.hotkeys = input::Hotkeys::register(&triggers, |id| {
            if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                app.borrow_mut().trigger(id);
            }
        });
    }

    fn on_app_activated(&mut self) {
        if let Some(wid) = front_window() {
            self.mru.touch(model::WindowId(wid));
            if self.config.background_capture {
                self.request_preview(wid);
            }
        }
        self.sync_hotkeys();
    }

    fn menu_command(&mut self, cmd: MenuCommand) {
        match cmd {
            MenuCommand::Settings => self.show_settings(),
            MenuCommand::TogglePause => self.toggle_pause(),
            MenuCommand::Restart => self.restart(),
            MenuCommand::Quit => self.quit(),
        }
    }

    fn show_settings(&mut self) {
        // The window holds its own copy of the config; hand it the current one
        // before showing, or an edit would save a snapshot from last time over
        // whatever has changed on disk since.
        self.strip.set_fade_out(self.config.animation.fade_out);
        if let Some(settings) = &self.settings {
            settings.set_config(&self.config);
        }
        if self.settings.is_none() {
            let path = self.config_path.clone();
            let settings = ui::Settings::new(self.mtm, &self.config, move |edited| {
                if let Err(err) = config::save(&edited, &path) {
                    println!("config: save failed — {err}");
                }
            });
            // Closing the window puts Oriel back to a menu-bar agent.
            settings.on_close(|| {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm)
                        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                }
            });
            self.settings = Some(settings);
        }
        // A settings window is a real window, so become a real app while it is
        // up: that is what puts Oriel in the Dock and in ⌘⇥, and what lets
        // choosing Settings again bring an already-open window forward instead
        // of ordering it front behind everything.
        let ns_app = NSApplication::sharedApplication(self.mtm);
        ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        if let Some(settings) = &self.settings {
            settings.show(self.mtm);
        }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            self.suppression = None;
            if self.session.is_some() {
                self.cancel();
            }
        } else {
            self.suppression = input::Suppression::engage();
            if self.suppression.is_none() {
                println!("input: native switcher not suppressed — it stays live alongside Oriel");
            }
        }
        if let Some(mb) = &self.menubar {
            mb.set_paused(self.paused);
        }
        self.sync_hotkeys();
    }

    fn quit(&mut self) {
        // Drop suppression before AppKit terminates so the native switcher is
        // restored even if Rust destructors do not run on process exit.
        self.hotkeys = None;
        self.suppression = None;
        NSApplication::sharedApplication(self.mtm).terminate(None);
    }

    /// Only exit once the replacement is actually running — quitting after a
    /// failed spawn would leave the user with no switcher at all.
    fn restart(&mut self) {
        let Ok(exe) = std::env::current_exe() else {
            println!("restart: cannot find the executable — staying up");
            return;
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        // Release the hot keys and the suppression first, so the replacement
        // can claim them; restore both if the spawn fails.
        self.hotkeys = None;
        self.suppression = None;
        match std::process::Command::new(exe).args(args).spawn() {
            Ok(_) => NSApplication::sharedApplication(self.mtm).terminate(None),
            Err(err) => {
                println!("restart: could not relaunch ({err}) — staying up");
                self.suppression = input::Suppression::engage();
                self.sync_hotkeys();
            }
        }
    }

    fn flush_pending_config(&mut self) {
        if let Some(cfg) = self.pending_config.take() {
            self.apply_config(cfg);
        }
    }

    fn apply_config(&mut self, cfg: config::Config) {
        if self.session.is_some() {
            self.pending_config = Some(cfg);
            return;
        }
        if cfg == self.config {
            return;
        }

        // Boot refuses to run with no usable lens; a reload must not sneak the
        // app into that state either. Suppression would stay engaged with
        // nothing registered to answer ⌘⇥, leaving no switcher at all.
        let bindings = lens::bindings_from_config(&cfg);
        if bindings.is_empty() {
            println!("config: reload ignored — no lens has a usable trigger");
            return;
        }

        // Editing the agent list has to reach the detector, and the samples
        // taken under the old list are not a measurement of anything under the
        // new one.
        if cfg.lantern.binaries != self.config.lantern.binaries {
            self.lantern = crate::lantern::Lantern::new(cfg.lantern.binaries.clone());
        } else if !cfg.lantern.enabled {
            self.lantern.reset();
        }

        let login_changed = cfg.start_at_login != self.config.start_at_login;
        let menubar_wanted = cfg.menubar_icon;
        let menubar_changed = menubar_wanted != self.config.menubar_icon;

        self.bindings = bindings;
        self.summon_delay_ms = cfg.summon_delay_ms.min(900);
        self.action_keys = ActionKeys::resolve(&cfg.keys);
        let hover_changed = cfg.controls.hover_select != self.controls.hover_select;
        self.controls = cfg.controls.clone();
        if hover_changed {
            self.strip.set_hover_select(self.controls.hover_select);
        }
        if let Some(settings) = &self.settings {
            settings.set_config(&self.config);
        }
        self.rules = config::to_model_rules(&cfg.rules);
        self.config = cfg;

        if login_changed {
            let _ = login::set_enabled(self.config.start_at_login);
        }

        if menubar_changed {
            if menubar_wanted {
                self.menubar = MenuBar::new(|cmd| {
                    if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                        app.borrow_mut().menu_command(cmd);
                    }
                });
                if let Some(mb) = &self.menubar {
                    mb.set_paused(self.paused);
                }
            } else {
                self.menubar = None;
            }
        }

        // Force a fresh registration against the new trigger set.
        self.hotkeys = None;
        self.sync_hotkeys();
    }
}

fn frontmost_pid() -> i32 {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map_or(0, |app| app.processIdentifier())
}

/// The window id of the current frontmost app's focused window, so the MRU can
/// track switches the user makes outside Oriel (a click, an app's shortcut).
fn front_window() -> Option<u32> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    ax::focused_window(app.processIdentifier())
}

fn install_session_taps(app: &Rc<RefCell<App>>) -> Option<(input::EventTap, input::EventTap)> {
    let on_key = app.clone();
    let key_mask = input::event_mask(&[CGEventType::KeyDown]);
    let Some(key_tap) =
        input::EventTap::install(CGEventTapOptions::Default, key_mask, move |_ty, ev| {
            on_key.borrow_mut().on_key(ev)
        })
    else {
        println!("input: could not install the session key tap (accessibility?)");
        return None;
    };
    let keys = key_tap.handle();
    keys.set_enabled(false);
    app.borrow_mut().keys = Some(keys);

    let on_flags = app.clone();
    let mask = input::event_mask(&[CGEventType::FlagsChanged]);
    let Some(flags_tap) =
        input::EventTap::install(CGEventTapOptions::ListenOnly, mask, move |_ty, ev| {
            on_flags.borrow_mut().flags_changed(input::flags(ev));
            input::Disposition::Keep
        })
    else {
        println!("input: could not install the release tap (accessibility?)");
        return None;
    };
    Some((key_tap, flags_tap))
}

struct ActivationHook {
    _observer:
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
    _block: block2::RcBlock<dyn Fn(core::ptr::NonNull<objc2_foundation::NSNotification>)>,
}

fn install_activation_observer(app: &Rc<RefCell<App>>) -> ActivationHook {
    let on_activate = app.clone();
    let block = block2::RcBlock::new(
        move |_: core::ptr::NonNull<objc2_foundation::NSNotification>| {
            on_activate.borrow_mut().on_app_activated();
        },
    );
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let observer = unsafe {
        workspace
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(objc2_app_kit::NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &block,
            )
    };
    ActivationHook {
        _observer: observer,
        _block: block,
    }
}

/// Mouse in the strip: click focuses, scroll moves the selection, hover
/// selects when the config asks for it. Each callback re-enters `App`, so none
/// of them may hold a borrow while calling in.
fn install_mouse(app: &Rc<RefCell<App>>) {
    let a = app.borrow();
    a.strip.on_click(|index| {
        if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
            app.borrow_mut().click_tile(index);
        }
    });
    a.strip.on_scroll(|delta| {
        if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
            app.borrow_mut().scroll_selection(delta);
        }
    });
    // Register unconditionally: the strip gates delivery on its own
    // `hover_select` flag, so this costs nothing while hover is off — and
    // turning it on later actually works, which a boot-time-only registration
    // silently prevented.
    a.strip.on_hover(|index| {
        if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
            app.borrow_mut().hover_tile(index);
        }
    });
    a.strip.set_hover_select(a.controls.hover_select);
}

fn boot_triggers(app: &Rc<RefCell<App>>, want_menubar: bool) -> bool {
    let mut a = app.borrow_mut();
    if want_menubar {
        a.menubar = MenuBar::new(|cmd| {
            if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                app.borrow_mut().menu_command(cmd);
            }
        });
    }
    a.sync_hotkeys();
    if a.hotkeys.is_some() || a.paused {
        return true;
    }
    let triggers = lens::triggers_for(&a.bindings);
    if triggers.is_empty() || a.frontmost_should_pass() {
        return true;
    }
    println!("input: could not register triggers");
    false
}

/// Boots the resident app: suppresses the native switcher, binds the triggers
/// and the release tap, then runs the main loop until quit.
pub fn run(mtm: MainThreadMarker, cfg: &config::Config) {
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let ws = match winsrv::WindowServer::connect() {
        Ok(ws) => ws,
        Err(missing) => {
            println!("skylight: unavailable — {}", missing.join(", "));
            return;
        }
    };

    let bindings = lens::bindings_from_config(cfg);
    if bindings.is_empty() {
        println!("input: no lenses registered");
        return;
    }
    let _ = login::set_enabled(cfg.start_at_login);
    let path = config_path();

    let app = Rc::new(RefCell::new(App {
        mtm,
        ws: Rc::new(ws),
        strip: ui::Strip::new(mtm),
        peek: ui::Peek::new(mtm),
        peek_stamp: Cell::new(0),
        peek_tx: None,
        session: None,
        bindings,
        summon_delay_ms: cfg.summon_delay_ms.min(900),
        #[cfg(debug_assertions)]
        triggered_at: None,
        show_epoch: 0,
        keys: None,
        action_keys: ActionKeys::resolve(&cfg.keys),
        controls: cfg.controls.clone(),
        held: CGEventFlags::empty(),
        generation: Rc::new(Cell::new(0)),
        mru: model::Mru::default(),
        cache: capture::Cache::new(PREVIEW_BUDGET),
        worker: None,
        config: cfg.clone(),
        config_path: path.clone(),
        rules: config::to_model_rules(&cfg.rules),
        bundles: HashMap::new(),
        hotkeys: None,
        suppression: None,
        paused: false,
        menubar: None,
        lit: Vec::new(),
        lantern: crate::lantern::Lantern::new(cfg.lantern.binaries.clone()),
        settings: None,
        pending_config: None,
    }));
    APP.with(|slot| *slot.borrow_mut() = Rc::downgrade(&app));
    spawn_capture(&app);
    schedule_lantern();
    spawn_peek_capture(&app);
    install_mouse(&app);
    app.borrow().strip.set_fade_out(cfg.animation.fade_out);

    if !boot_triggers(&app, cfg.menubar_icon) {
        return;
    }
    let Some(_taps) = install_session_taps(&app) else {
        return;
    };
    let _observer = install_activation_observer(&app);

    reload::spawn(path, |cfg| {
        on_main(move || {
            if let Some(app) = APP.with(|slot| slot.borrow().upgrade()) {
                app.borrow_mut().apply_config(cfg);
            }
        });
    });

    let suppression = input::Suppression::engage();
    if suppression.is_none() {
        println!("input: native switcher not suppressed — it stays live alongside Oriel");
    }
    app.borrow_mut().suppression = suppression;

    ns_app.run();
}
