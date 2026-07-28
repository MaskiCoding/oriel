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
    model::WindowId(pid.cast_unsigned() | 0x8000_0000)
}

fn switchable(w: &winsrv::WindowInfo) -> bool {
    w.level == 0 && model::WindowState::decode(w.tags, w.attributes).switchable()
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
fn screen_frames() -> Vec<(f64, f64, f64, f64)> {
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

    let ids: Vec<model::WindowId> = windows.iter().map(|w| model::WindowId(w.wid)).collect();
    mru.sync(&ids);
    let mut by_wid: HashMap<u32, winsrv::WindowInfo> =
        windows.into_iter().map(|w| (w.wid, w)).collect();
    let ordered: Vec<winsrv::WindowInfo> = mru
        .order()
        .iter()
        .filter_map(|id| by_wid.remove(&id.0))
        .collect();

    let frames = screen_frames();
    let mut seen_apps = HashSet::new();
    let mut out_windows = Vec::with_capacity(ordered.len());
    let mut meta = HashMap::with_capacity(ordered.len());
    let mut owned_pids = HashSet::new();

    for w in ordered {
        // Count the app as window-owning even when every window is filtered out,
        // so we do not invent a windowless tile for it below.
        owned_pids.insert(w.pid);
        let title = w.title.clone().unwrap_or_default();
        if hidden_by_rule(rules, bundles, w.pid, &title, true) {
            continue;
        }
        let state = model::WindowState::decode(w.tags, w.attributes);
        let space = if map.uniform() {
            None
        } else {
            ws.window_space(w.wid)
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
            app_name: w.app.unwrap_or_default(),
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

    for (pid, app_name) in windowless_apps(&owned_pids) {
        if hidden_by_rule(rules, bundles, pid, "", false) {
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
            title: String::new(),
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

    Snapshot {
        windows: out_windows,
        meta,
    }
}
