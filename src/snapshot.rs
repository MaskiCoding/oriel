//! Full window snapshot for the lens resolver: MRU-ordered `model::Window`
//! records plus the pid/aspect/badge metadata the UI layer still needs.

use std::collections::{HashMap, HashSet};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSScreen, NSWorkspace};

/// Parallel lookup for the pid/aspect/badge the UI layer needs, keyed by window id.
pub struct Meta {
    pub pid: i32,
    pub aspect: f64,
    pub badge: String,
}

pub struct Snapshot {
    pub windows: Vec<model::Window>,
    pub meta: HashMap<model::WindowId, Meta>,
}

const FALLBACK_ASPECT: f64 = 1.6;

/// Synthetic ids for windowless apps: high bit set so they cannot collide with
/// real `CGWindowID`s, which are allocated from a low monotonic counter.
fn windowless_id(pid: i32) -> model::WindowId {
    model::WindowId(pid.cast_unsigned() | WINDOWLESS_BIT)
}

/// High bit marks a synthetic windowless-app entry; real `CGWindowID`s come
/// from a low monotonic counter and never reach it.
pub const WINDOWLESS_BIT: u32 = 0x8000_0000;

/// Whether `wid` is a synthetic windowless-app entry rather than a real window.
pub fn is_windowless(wid: u32) -> bool {
    wid & WINDOWLESS_BIT != 0
}

fn switchable(w: &winsrv::WindowInfo) -> bool {
    w.level == 0
        && model::WindowState::decode(w.tags, w.attributes).switchable()
        && model::WindowState::meets_min_size(w.width, w.height)
}

fn aspect_of(w: &winsrv::WindowInfo) -> f64 {
    if w.width > 0.0 && w.height > 0.0 {
        w.width / w.height
    } else {
        FALLBACK_ASPECT
    }
}

/// `AppKit` screen frames converted to the `WindowServer`'s top-left coordinate
/// space so they can be compared with `WindowInfo::{x,y,width,height}`.
pub(crate) fn screen_frames() -> Vec<(f64, f64, f64, f64)> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.firstObject() else {
        return Vec::new();
    };
    let primary_h = primary.frame().size.height;
    screens
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            let origin_x = frame.origin.x;
            let origin_y = frame.origin.y;
            let width = frame.size.width;
            let height = frame.size.height;
            (origin_x, primary_h - origin_y - height, width, height)
        })
        .collect()
}

struct SpaceMark {
    fullscreen: bool,
    current: bool,
    ordinal: u32,
}

/// Mirrors `SpaceMap`'s desktop numbering (user Spaces only) for the fields
/// `SpaceMap` does not expose publicly.
fn space_marks(spaces: &[winsrv::SpaceInfo]) -> HashMap<u64, SpaceMark> {
    let mut desktop = 0_u32;
    let mut marks = HashMap::new();
    for s in spaces {
        if !s.fullscreen {
            desktop = desktop.saturating_add(1);
        }
        marks.insert(
            s.id,
            SpaceMark {
                fullscreen: s.fullscreen,
                current: s.current,
                ordinal: desktop,
            },
        );
    }
    marks
}

/// Every app that could own a window. Lantern attributes an agent's work to the
/// nearest of these above it in the process tree.
pub fn app_pids() -> Vec<model::Pid> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        // Only apps that can own a window. Attribution takes the *nearest*
        // ancestor, and helper processes — Electron's renderers, an editor's
        // language servers — sit between the shell and the bundle that actually
        // owns the tile. Counting them means work is keyed to a pid no window
        // has, so the menu-bar count rises while every tile stays dark.
        .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid > 0)
        .map(model::Pid)
        .collect()
}

fn windowless_apps(owned: &HashSet<i32>) -> Vec<(i32, String)> {
    let workspace = NSWorkspace::sharedWorkspace();
    let mut out = Vec::new();
    for app in workspace.runningApplications() {
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let pid = app.processIdentifier();
        if owned.contains(&pid) {
            continue;
        }
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        out.push((pid, name));
    }
    out
}

/// Bundle ID for `pid`, cached across summons. `None` means a bare executable
/// with no bundle — it matches no prefix rule.
pub fn bundle_id(pid: i32, cache: &mut HashMap<i32, Option<String>>) -> Option<&str> {
    cache.entry(pid).or_insert_with(|| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .and_then(|app| app.bundleIdentifier())
            .map(|s| s.to_string())
    });
    cache.get(&pid).and_then(Option::as_deref)
}

/// Drops cache entries for processes that are gone. macOS reuses pids, so a
/// stale entry would eventually hand one app's rules to an unrelated one.
pub fn prune_bundles(cache: &mut HashMap<i32, Option<String>>, alive: &HashSet<i32>) {
    cache.retain(|pid, _| alive.contains(pid));
}

fn hidden_by_rule(
    rules: &model::Rules,
    bundles: &mut HashMap<i32, Option<String>>,
    pid: i32,
    title: &str,
    has_open_window: bool,
) -> bool {
    let Some(bid) = bundle_id(pid, bundles) else {
        return false;
    };
    rules.should_hide(bid, title, has_open_window)
}

/// Debug / warm path without a live rules handle — applies shipped defaults.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn snapshot(ws: &winsrv::WindowServer, mru: &mut model::Mru) -> Snapshot {
    let rules = model::Rules::defaults();
    let mut bundles = HashMap::new();
    snapshot_with(ws, mru, &rules, &mut bundles)
}

/// Adds one synthetic entry per running app that owns no window (PRD §4.3),
/// and prunes the bundle-ID cache to processes that still exist.
fn append_windowless(
    rules: &model::Rules,
    bundles: &mut HashMap<i32, Option<String>>,
    owned_pids: &HashSet<i32>,
    out_windows: &mut Vec<model::Window>,
    meta: &mut HashMap<model::WindowId, Meta>,
) {
    let running = windowless_apps(owned_pids);
    // Everything alive right now: windows we saw, plus every running app.
    let mut alive: HashSet<i32> = owned_pids.clone();
    alive.extend(running.iter().map(|(pid, _)| *pid));
    prune_bundles(bundles, &alive);

    for (pid, app_name) in running {
        let title = model::WindowState::tile_title(None, None, &app_name);
        if hidden_by_rule(rules, bundles, pid, &title, false) {
            continue;
        }
        let id = windowless_id(pid);
        meta.insert(
            id,
            Meta {
                pid,
                aspect: FALLBACK_ASPECT,
                badge: String::new(),
            },
        );
        out_windows.push(model::Window {
            id,
            app: pid,
            app_name,
            title,
            state: model::WindowState::decode(0, 0),
            fullscreen: false,
            space: None,
            space_visible: true,
            space_ordinal: 0,
            screen: 0,
            created: u64::from(id.0),
            is_main: true,
            windowless: true,
        });
    }
}

pub fn snapshot_with(
    ws: &winsrv::WindowServer,
    mru: &mut model::Mru,
    rules: &model::Rules,
    bundles: &mut HashMap<i32, Option<String>>,
) -> Snapshot {
    let spaces = ws.spaces();
    let map = model::SpaceMap::new(spaces.iter().map(|s| model::SpaceDesc {
        id: s.id,
        current: s.current,
        fullscreen: s.fullscreen,
    }));
    let marks = space_marks(&spaces);
    let space_ids: Vec<u64> = spaces.iter().map(|s| s.id).collect();
    let mut windows = ws.windows(&space_ids);
    windows.retain(switchable);
    // Titles only for what survived: two thirds of the queried rows are ghosts
    // or off-level windows that will never be drawn.
    ws.fill_titles(&mut windows);

    let ids: Vec<model::WindowId> = windows.iter().map(|w| model::WindowId(w.wid)).collect();
    mru.sync(&ids);
    let mut by_wid: HashMap<u32, winsrv::WindowInfo> =
        windows.into_iter().map(|w| (w.wid, w)).collect();
    let ordered: Vec<winsrv::WindowInfo> = mru
        .order()
        .iter()
        .filter_map(|id| by_wid.remove(&id.0))
        .collect();

    // One round trip per Space, not per window — only when a badge is possible.
    let by_space = if map.uniform() {
        HashMap::new()
    } else {
        ws.windows_by_space(&space_ids)
    };
    let frames = screen_frames();
    let mut seen_apps = HashSet::new();
    let mut out_windows = Vec::with_capacity(ordered.len());
    let mut meta = HashMap::with_capacity(ordered.len());
    let mut owned_pids = HashSet::new();

    for w in ordered {
        // Count the app as window-owning even when every window is filtered out,
        // so we do not invent a windowless tile for it below.
        owned_pids.insert(w.pid);
        let app_name = w.app.clone().unwrap_or_default();
        let title = model::WindowState::tile_title(None, w.title.as_deref(), &app_name);
        if hidden_by_rule(rules, bundles, w.pid, &title, true) {
            continue;
        }
        let state = model::WindowState::decode(w.tags, w.attributes);
        let space = if map.uniform() {
            None
        } else {
            by_space.get(&w.wid).copied()
        };
        let mark = space.and_then(|id| marks.get(&id));
        let is_main = seen_apps.insert(w.pid);
        let id = model::WindowId(w.wid);
        meta.insert(
            id,
            Meta {
                pid: w.pid,
                aspect: aspect_of(&w),
                badge: map.badge(state.minimized, space),
            },
        );
        out_windows.push(model::Window {
            id,
            app: w.pid,
            app_name,
            title,
            state,
            fullscreen: mark.is_some_and(|m| m.fullscreen),
            space,
            space_visible: mark.is_some_and(|m| m.current) || (map.uniform() && space.is_none()),
            space_ordinal: mark.map_or(0, |m| m.ordinal),
            screen: winsrv::screen_index((w.x, w.y, w.width, w.height), &frames),
            // WindowServer exposes no creation time; ids are allocated monotonically.
            created: u64::from(w.wid),
            is_main,
            windowless: false,
        });
    }

    append_windowless(rules, bundles, &owned_pids, &mut out_windows, &mut meta);

    Snapshot {
        windows: out_windows,
        meta,
    }
}
