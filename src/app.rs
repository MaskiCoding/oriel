//! The M1 loop: hold a trigger, cycle with taps, release to jump.

#[path = "keys.rs"]
mod keys;

use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak};

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGEventFlags, CGEventTapOptions, CGEventType, CGImage};

use crate::lens::{self, Binding};
use keys::{ActionKeys, Dir, KEY_DELETE};

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

fn tile_of(c: &Candidate, preview: Option<CFRetained<CGImage>>) -> ui::Tile {
    ui::Tile {
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
    w.level == 0 && model::WindowState::decode(w.tags, w.attributes).switchable()
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

/// The preview cache's hard byte budget; at tile-sized thumbnails this is on
/// the order of a hundred windows.
const PREVIEW_BUDGET: usize = 32 << 20;

struct Live {
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
    ws: Rc<winsrv::WindowServer>,
    strip: ui::Strip,
    session: Option<Live>,
    bindings: Vec<Binding>,
    summon_delay_ms: u32,
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
}

impl App {
    fn binding(&self, idx: usize) -> Option<&Binding> {
        self.bindings.get(idx)
    }

    /// A trigger hot key fired. Opens a session on the first press; once open,
    /// the same lens is ignored (cycling is the key tap's job) and a different
    /// lens morphs the strip in place.
    fn trigger(&mut self, id: u32) {
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
        let candidates = self.enumerate(&binding);
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            return;
        };
        if backward {
            selection.select(candidates.len() - 1);
        }

        let epoch = self.show_epoch;
        self.session = Some(Live {
            candidates,
            selection,
            binding_idx: idx,
            shown: false,
            show_epoch: epoch,
            filter: None,
        });
        self.strip.set_look(binding.look);

        let linger = lens::stays_open(binding.on_release);
        let held = binding.hold.is_empty() || self.held.contains(binding.hold);

        // Hot-key dispatch lags the flags tap, so the trigger modifier may
        // already be up by now — a flick too quick to leave a release event.
        // Jump straight away without ever showing the strip in that case.
        if !linger && !held {
            self.jump();
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
        let candidates = self.enumerate(&binding);
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
            candidates,
            selection,
            binding_idx: idx,
            shown,
            show_epoch,
            filter: None,
        });
        self.strip.set_look(binding.look);
        if shown {
            self.render();
        }
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
        self.set_keys_enabled(true);
        self.render();
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

    fn move_sel(&mut self, dir: Dir) {
        let Some(live) = &self.session else {
            return;
        };
        let n = live.candidates.len();
        let selected = live.selection.selected();
        let style = self
            .binding(live.binding_idx)
            .map_or(ui::Style::Gallery, |b| b.look.style);
        let cols = keys::grid_cols(n, style);
        let next = keys::step(selected, n, cols, dir);
        if let Some(live) = &mut self.session {
            live.selection.select(next);
        }
        self.strip.select(next);
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
        let candidates = self.enumerate(&binding);
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
        let Some(binding) = self.binding(binding_idx).cloned() else {
            return;
        };
        let base = self.enumerate(&binding);
        if base.is_empty() {
            self.cancel();
            return;
        }
        let candidates = rank_filter(base, &query);
        let Some(mut selection) = model::Session::start(candidates.len()) else {
            self.cancel();
            return;
        };
        selection.select(0);
        self.session = Some(Live {
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
            let _ = ax::close_window(pid, wid);
        }
        self.refresh_candidates();
    }

    fn act_minimize(&mut self) {
        if let Some((pid, wid)) = self.selected_target() {
            let next = !ax::is_minimized(pid, wid).unwrap_or(false);
            let _ = ax::set_minimized(pid, wid, next);
        }
        self.refresh_candidates();
    }

    fn act_fullscreen(&mut self) {
        if let Some((pid, wid)) = self.selected_target() {
            let next = !ax::is_fullscreen(pid, wid).unwrap_or(false);
            let _ = ax::set_fullscreen(pid, wid, next);
        }
        self.refresh_candidates();
    }

    fn act_quit(&mut self) {
        if let Some((pid, _)) = self.selected_target() {
            quit_pid(pid);
        }
        self.refresh_candidates();
    }

    fn act_hide(&mut self) {
        if let Some((pid, _)) = self.selected_target() {
            let next = !ax::is_app_hidden(pid).unwrap_or(false);
            let _ = ax::set_app_hidden(pid, next);
        }
        self.refresh_candidates();
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
        let base = self.enumerate(&binding);
        if base.is_empty() {
            self.cancel();
            return;
        }
        let candidates = match &filter {
            Some(query) => rank_filter(base, query),
            None => base,
        };
        if candidates.is_empty() {
            self.cancel();
            return;
        }
        let mut selection = model::Session::start(candidates.len()).unwrap();
        selection.select(selected.min(candidates.len() - 1));
        self.session = Some(Live {
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
        self.session = None;
        self.strip.set_query(None);
        self.strip.hide();
        self.set_keys_enabled(false);
    }

    /// Ends the session and focuses the current selection, deferred briefly so
    /// the focus lands after the triggering event settles (see `on_main_after`).
    fn jump(&mut self) {
        self.show_epoch = self.show_epoch.wrapping_add(1);
        let Some(live) = self.session.take() else {
            return;
        };
        self.strip.set_query(None);
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

    fn enumerate(&mut self, binding: &Binding) -> Vec<Candidate> {
        let snap = crate::snapshot::snapshot(&self.ws, &mut self.mru);
        let ids: Vec<model::WindowId> = snap.windows.iter().map(|w| w.id).collect();
        self.cache.retain(|wid| ids.contains(&model::WindowId(wid)));

        let ctx = model::ResolveCtx {
            active_app: frontmost_pid(),
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

    /// Paints the strip for the current session: previews straight from cache
    /// (stale beats late — first paint never waits on a capture), then a
    /// refresh of every visible window queued behind it.
    fn render(&mut self) {
        let Some(live) = &self.session else {
            return;
        };
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
        let Some(live) = &self.session else {
            return;
        };
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
        // Synthetic windowless ids are not capturable.
        if wid & 0x8000_0000 != 0 {
            return;
        }
        if let Some(worker) = &self.worker {
            worker.request(wid);
        }
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
    let summon_delay_ms = cfg.summon_delay_ms.min(900);
    let triggers = lens::triggers_for(&bindings);
    let action_keys = ActionKeys::resolve(&cfg.keys);
    let controls = cfg.controls.clone();

    let strip = ui::Strip::new(mtm);
    let app = Rc::new(RefCell::new(App {
        ws: Rc::new(ws),
        strip,
        session: None,
        bindings,
        summon_delay_ms,
        show_epoch: 0,
        keys: None,
        action_keys,
        controls,
        held: CGEventFlags::empty(),
        generation: Rc::new(Cell::new(0)),
        mru: model::Mru::default(),
        cache: capture::Cache::new(PREVIEW_BUDGET),
        worker: None,
    }));
    APP.with(|slot| *slot.borrow_mut() = Rc::downgrade(&app));
    spawn_capture(&app);

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
