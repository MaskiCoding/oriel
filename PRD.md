# Oriel — PRD

A keyboard-driven window switcher for macOS, written in Rust. Hold a modifier, tap a key, see every window as a live tile, release to jump. Fast, native-looking, resident in the menu bar, configured by a file.

An oriel is a bay window that projects outward so you can see in every direction at once. Same idea.

---

## 1. Goals

- **Window-level switching.** The unit of switching is the *window*, not the app. Every window from every app, every Space, every screen, in one strip, ordered by global recency of focus.
- **Muscle-memory first.** Two triggers from day one: `⌘⇥` for everything, `⌥⇥` scoped to the active app. Hold-cycle-release must feel identical to what the fingers already know.
- **Instant.** First paint of the strip ≤ 33 ms (~two frames) from keypress. The panel is resident, the window model is always warm, and previews paint from cache — summoning does zero enumeration, zero capture, zero window creation. A quick flick still switches to the previous window without engaging the UI.
- **Light.** Menu-bar-only resident app. Target: < 40 MB RSS with background capture off, < 120 MB steady-state with live thumbnails on. One process, no helpers.
- **Free and quiet.** No license checks, no telemetry, no crash-report uploads, no network calls at all. English only.

## 2. Non-goals

- Not a window *manager* — no tiling, moving, or resizing.
- No App Store distribution (the implementation requires private system APIs; see §6).
- No support for macOS older than 26. This is a personal tool for current-OS machines.
- No localization, no auto-updater, no onboarding wizard. Updates ship via `git pull && just install`.

## 3. Concepts & vocabulary

| Term | Meaning |
|---|---|
| **Lens** | A trigger plus a scope plus a look. Each lens = hold-modifier + key, its own filters (which windows), ordering, and presentation. Up to 9 lenses. |
| **Strip** | The overlay panel showing window tiles. Non-activating: summoning it never deactivates the app under it. |
| **Tile** | One entry in the strip: live preview (or icon/title), app icon badge, state markers (minimized ●, fullscreen ⤢, Space number). |
| **Jump / Linger / Filter** | What happens when the held modifier is released. **Jump**: focus the selection. **Linger**: strip stays open for mouse/arrow browsing. **Filter**: strip stays open with a query field focused — type to narrow. |
| **Peek** | Optional full-size live preview of the selected window, shown behind/beside the strip. |
| **App rules** | Per-app overrides keyed by bundle-ID prefix: hide an app's windows from the strip, or pass triggers through to the app (essential for VMs and remote desktops). |

## 4. Functional requirements

### 4.1 Session lifecycle

- Hold modifier + press key → session starts. Strip appears immediately (apparition delay defaults to **0 ms**; a 0–900 ms slider exists for those who prefer quick flicks to never flash the UI). A flick — release before/at first paint — jumps to the previous window either way.
- Repeated key presses (and key-repeat) cycle forward; `⇧` cycles backward; once a session is active, the bare trigger key keeps cycling.
- Release of the held modifier executes the lens's release action (Jump/Linger/Filter). `⏎` focuses, `⎋` cancels, click focuses.
- Selection restores correct state: focusing a minimized window de-minimizes it; a hidden app unhides; a window on another Space switches Spaces; an app with no windows gets activated/launched.
- The native system app switcher is suppressed while Oriel holds `⌘⇥`-family triggers, and **always restored on quit or crash** (the suppression persists at the OS level otherwise — this is a hard invariant).

### 4.2 In-session controls (rebindable single keys)

| Default | Action |
|---|---|
| `←→↑↓` / `hjkl` | Move selection |
| `⏎` | Focus selection |
| `⎋` | Cancel (exits Filter first, then the session) |
| `W` | Close selected window |
| `M` | Minimize / restore |
| `F` | Fullscreen toggle |
| `Q` | Quit selected app |
| `H` | Hide / show selected app |
| typing (Filter mode) | Narrow the strip |

Mouse: click to focus; optional hover-select; scroll to pan the strip. (Drag-a-file-onto-a-tile-to-open: backlog, not v1.)

### 4.3 Per-lens filtering

Each lens declares:

- **Apps**: all / active app only / all except active app
- **Spaces**: all / visible only / non-visible only
- **Screens**: all / the screen showing the strip
- **Minimized / hidden / fullscreen windows**: show / show at end / hide (each independently)
- **Apps with no open window**: show / show at end / hide

Default lenses: Lens 1 `⌘⇥` all apps, everything shown, windowless apps at end. Lens 2 `⌥⇥` active app only.

### 4.4 Ordering & grouping (per lens)

- Order: **recent focus** (default, global MRU maintained continuously from focus events, not just Oriel's own switches) / creation time / alphabetical / Space order.
- Grouping: all windows separately (default) or one tile per app (its main window).
- Native window tabs collapse to one tile by default (option: tile per tab).

### 4.5 Filter (type-to-narrow)

Match tiers, best first: exact > prefix > word-boundary prefix (incl. camelCase) > substring > acronym > small-edit-distance fuzzy. App name weighs above window title. Diacritic-insensitive. Matched spans highlighted. Matches sort above non-matches rather than hiding them instantly (no layout jumps mid-typing).

### 4.6 Presentation

- **Styles** (per lens): **Gallery** (live preview tiles — default), **Icons** (large app icons, compact), **List** (rows of icon + title, dense).
- **Size**: small / medium / large / **auto** (few windows → bigger tiles, many → more per row; computed from physical screen dimensions).
- **Theme**: light / dark / system. Native vibrancy/blur material.
- **Multi-screen**: show strip on active screen (default) / screen with pointer / screen with menu bar.
- **Peek**: off by default; when on, selection shows a full-size preview.
- Titles show window title / app name / both; truncate start/middle/end. State markers and Space badges can be hidden.
- Animations minimal: apparition delay slider (0–900 ms, default 0), optional fade-out. Nothing else moves. Nothing ever animates *in*.

### 4.7 App rules

- Rule = bundle-ID **prefix** match (e.g. `com.parallels.` covers every Parallels executable).
- Per rule: hide windows (never / always / when app has no open window / when title contains any of N substrings) and pass triggers through (never / always / when the app is fullscreen).
- Shipped defaults: Finder hidden when it has no open window; trigger pass-through-when-fullscreen for the usual capture-everything suspects (Screen Sharing, Microsoft Remote Desktop, TeamViewer, VirtualBox, `com.parallels.`, Citrix, VMware Fusion, UTM).

### 4.8 Resident app surface

- Menu bar item (hideable): open settings, pause Oriel, restart, quit.
- Config is a **TOML file** at `~/.config/oriel/config.toml` — the source of truth, hot-reloaded on save, diffable, versionable. A native settings window comes later (M5) and is a *view* over the same file.
- Start at login via the modern service-management API. `LSUIElement` — no Dock icon.
- Permissions bootstrap on first run: Accessibility is required (hard gate with a guided prompt); Screen Recording is optional — without it Oriel runs in Icons/List styles with previews disabled, and says so once, quietly.
- **Background capture** toggle (default on): keeps previews warm so the strip opens pre-populated, at the cost of the OS screen-capture indicator staying lit and slightly higher RSS. Off = capture only during a session.

### 4.9 Backlog (explicitly not v1)

Trackpad gestures (3/4-finger swipe trigger), CLI (`oriel list --json`, `oriel focus <id>`), drag-and-drop onto tiles, per-lens theme overrides, haptic feedback.

## 5. Architecture

### 5.1 Workspace layout

Granular crates, one concern each; unsafe FFI quarantined at the bottom:

```
oriel/
├── Cargo.toml            # workspace
├── crates/
│   ├── skylight-sys/     # extern "C" decls for private SkyLight/CGS symbols + build.rs framework link. 100% unsafe, zero logic.
│   ├── ax/               # safe RAII wrapper over the C Accessibility API (elements, attributes, actions, observers, trust check)
│   ├── model/            # pure state: Window/App/Space types, MRU, filter/order/group/match resolvers. No FFI, no AppKit — fully unit-tested.
│   ├── winsrv/           # WindowServer integration: enumeration, batched state queries, event tap on WS notifications, Space topology
│   ├── capture/          # preview pipeline: ScreenCaptureKit screenshots, private capture fallback for minimized/off-Space windows, throttling, cache budget
│   ├── input/            # trigger engine: symbolic-hotkey suppression, flagsChanged session tap, key capture, shortcut state machine, in-session keymap
│   ├── ui/               # the strip + peek: NSPanel via objc2-app-kit, tile layer tree, vibrancy, theming; later the settings window
│   └── config/           # TOML schema, defaults, hot-reload watcher
└── src/main.rs           # thin binary: wiring, menu bar, permissions bootstrap, lifecycle
```

### 5.2 Data flow

WindowServer is the source of truth; Accessibility is for actions and a few reads.

1. **Enumerate** window IDs across all Spaces via `CGSCopyWindowsWithOptionsAndTags` (z-ordered), plus `SLSGetOnScreenWindowList` for the current-Space seed.
2. **Batch-read state** (one IPC for N windows) via the `SLSWindowQueryWindows` iterator: id, pid, attributes, level, Space mask, title, bounds. Always off the main thread.
3. **Watch** via `SLSRegisterConnectionNotifyProc` + `SLSRequestNotificationsForWindows` (created/destroyed/moved/ordered/focused/Space-changed event IDs), plus workspace notifications for app activate/hide. No per-window AX observers — they leak and lie under load.
4. **Correlate** to Accessibility elements lazily per window: element→id via the private `_AXUIElementGetWindow`; id→element by scanning the app's `AXWindows`, or for off-Space windows by remote-token construction, time-boxed. Cache on the window; self-heal stale elements.
5. **Discriminate** real windows by AX role/subrole + minimum size; window-title fallback chain AX → WS → app name. Minimized state reads from AX only (the WS bit conflates minimize/hide/off-Space).

### 5.3 Focus sequence (the hard part)

Public activation APIs are advisory-only on modern macOS; cross-app focus needs the private sequence, in order:

1. Un-minimize via AX if needed.
2. `_SLPSSetFrontProcessWithOptions(psn, wid, userGenerated)` — fronts the process *and* raises the target window; triggers the Space switch for off-Space targets.
3. Make it key by posting a synthetic mouse down/up event record (`SLPSPostEventRecordTo`, hand-packed byte layout, click point just off the content at (-1,-1)).
4. `AXRaise` within the app's own stack; retry once through re-resolution if the element went stale.
5. For cross-Space jumps, restore the origin Space's front process via `SLSSpaceSetFrontPSN` so returning there doesn't surface the wrong app.

Record the intended target before step 2 so the resulting activation event bumps exactly that window in the MRU.

### 5.4 Trigger engine

- Suppress the native switcher's symbolic hotkeys (`⌘⇥` = 1, `⌘⇧⇥` = 2, `⌘\`` = 6) via `CGSSetSymbolicHotKeyEnabled` while bound; re-enable in every exit path including signal handlers.
- **Hold tracking**: a permanent *listen-only, flagsChanged-only* CGEventTap on the session tap. Listen-only + flags-only means Secure Input can't block it and input methods never see interference.
- **Trigger keys**: Carbon hotkey registration (works through Secure Input) for the combo press/release; a narrowly-scoped absorbing keyDown tap exists only while a session is active (swallow Tab/Esc/single-key actions from the focused app).
- Defensive: re-enable taps on timeout events; double-check actual modifier state on release (the OS can drop or reorder events).

### 5.5 Preview pipeline

- Primary: ScreenCaptureKit screenshot API per window (BGRA, cursor off, scaled to tile size; full-res only when Peek is on), on a bounded queue, ≤ 1 capture per window per 200 ms.
- Minimized and off-Space windows: private `CGSHWCaptureWindowList` (the only thing that can see them).
- Tile contents are pixel buffers handed straight to layer contents — no CPU-side conversion.
- **Stale-then-refresh**: summon always paints from cache first (a slightly old preview beats a late one); captures refresh tiles asynchronously after the strip is visible. First paint never blocks on a screenshot.
- **App icons**: read from the system's already-decoded running-app cache (never LaunchServices path lookups or `.icns` decoding), snapshotted once per app at launch-notification time, rasterized at tile size, keyed by bundle ID. Icon work never happens during summon.
- Cache with a hard byte budget; evict least-recently-shown. Drain in-flight captures before exit (prevents OS permission-dialog weirdness).

### 5.6 UI

**Decision: the viewport is native AppKit/CoreAnimation, driven from Rust via `objc2`.** The pixels are drawn by the same system frameworks a Swift app would use — Rust is just the language holding the steering wheel. A Swift layer was evaluated and rejected: it adds an FFI boundary, a second toolchain, and a slower iteration loop while producing the identical panel. Fallback if AppKit-from-Rust proves too grindy: `gpui` (its popup window kind is exactly a non-activating panel; Metal-rendered, proven in shipped switcher-like tools) — the crate split above makes `ui/` swappable without touching anything else.

- Strip = `NSPanel`, non-activating style mask, floating, `canJoinAllSpaces` + fullscreen-auxiliary collection behavior, popup-menu window level, clear background over a vibrancy view.
- **Resident panel**: created once at launch and kept alive; summon = update tiles + order-front, dismiss = order-out. The panel is never torn down or rebuilt on the hot path.
- Excluded from its own enumeration and capture; re-asserts key status while a session is active; app menu disabled while the panel is key so `⌘Q`-style keys can't hit Oriel itself.
- Tile grid is a recycled layer tree inside a scroll view; updates batched per transaction.

### 5.7 Threading

Main thread owns AppKit and the model. Everything that does IPC (WS queries, AX calls, captures) runs on dedicated bounded queues and hops results back to main. Event taps live on their own run-loop threads. AX messaging timeout lowered to 1 s so a beach-balling app can't stall the pipeline; per-app isolation so one bad citizen doesn't block the rest. Idle-sleep-friendly, App Nap disabled.

### 5.8 Stack

`objc2` + `objc2-foundation` / `objc2-app-kit` / `objc2-quartz-core` / `objc2-core-graphics` / `objc2-application-services` + `block2` / `dispatch2`; `screencapturekit` crate for captures; `tray-icon` + `muda` for the menu bar; `serde` + `toml` + `notify` for config; in-house `skylight-sys` for the ~15 private symbols. No webview, no game-engine UI, no async runtime.

## 5.9 Dev workflow

- **Toolchain**: pinned via `rust-toolchain.toml`. Format with stock `rustfmt`; lint with clippy configured in `[workspace.lints.clippy]`, warnings denied (FFI crates get targeted allows, never workspace-wide loosening).
- **`just check`** = `fmt --check` + `clippy -D warnings` + `cargo test` — the gate for every commit, enforced by a repo-committed pre-commit hook (`core.hooksPath`).
- **CI (GitHub Actions)**: one workflow, triggered on `pull_request` and `push` to `main`. Jobs: lint + pure-crate tests (`model`, `config`) on a Linux runner (cheap, fast), full workspace build + test on a macOS runner (the AppKit/FFI crates only compile there). Green CI = mergeable; there is deliberately no deploy stage — releases are local builds.
- **Branching**: direct pushes to `main` stay allowed (solo repo); the normal route is branch → PR → squash or rebase merge. Repo is configured to auto-delete head branches on merge, auto-merge enabled, merge commits disabled.
- **Tests that catch breakage**: every pure resolver (filtering, ordering, matching, shortcut state machine, config parsing) is unit-tested in `model`/`config`; integration smoke test boots the app headless and asserts the native-switcher-restore invariant (§8.4). FFI crates stay logic-free precisely so the untestable surface is minimal.

## 6. Constraints & risks

| Risk | Mitigation |
|---|---|
| Private SkyLight/CGS symbols shift between macOS releases | Feature-detect every symbol at startup (`dlsym`); degrade per-capability (worst case: icon tiles, current-Space only) instead of crashing. Isolate all of it in `skylight-sys`. |
| TCC permissions reset on every rebuild with ad-hoc signing | First thing in M0: create a **stable self-signed signing cert** and sign every dev build with it — grants then persist. Also: taps die silently if the running binary's signature changes; restart after re-sign. |
| Screen-capture indicator + periodic OS re-authorization prompts | Background capture is a user toggle; degraded icon-mode is first-class, not an afterthought. |
| Memory creep from preview cache | Hard byte budget + eviction; RSS targets in §1 are acceptance criteria, not aspirations. |
| One dev, many moving parts | `model/` is pure and unit-tested (filter/order/match/shortcut state machines); FFI crates stay logic-free; small commits, each leaving the app runnable. |

Sandbox off, library validation off (required for private frameworks) — which is also why App Store distribution is out of scope.

## 7. Milestones

Each milestone ends with something usable daily. Small commits throughout; comments only where the code can't speak (byte layouts, OS quirks).

- **M0 — Skeleton that sees.** Workspace scaffold, signing cert + bundle + permissions bootstrap, `skylight-sys`, enumeration + batched state query, `oriel` logs every window on all Spaces. *Exit: correct window list survives Space switches and app churn.*
- **M1 — The loop.** Trigger engine (`⌘⇥` + `⌥⇥`), MRU model, strip with icon+title tiles, cycle, Jump on release, quick-flick, native-switcher suppression with guaranteed restore. *Exit: daily-driveable; the old tool comes off login items.*
- **M2 — Eyes.** Preview pipeline, Gallery style, state markers, Space badges, minimized/off-Space capture, degraded no-permission mode. *Exit: Gallery is the default and feels instant.*
- **M3 — Lenses & Filter.** Full lens config (filters, ordering, grouping, per-lens style), Filter mode with tiered matching, Linger, in-session action keys (W/M/F/Q/H, arrows, vim). *Exit: both default lenses + typing-to-find work.*
- **M4 — Manners.** App rules + shipped defaults, multi-screen behavior, auto size, Peek, hot-reloaded config, menu bar, login item. *Exit: feature-complete against §4 minus backlog.*
- **M5 — Comfort.** Native settings window over the TOML, animations polish, edge-case hardening (Electron quirks, tab grouping, stale-element healing). *Exit: v1.0.*

## 8. Acceptance criteria

1. `⌘⇥` and `⌥⇥` behave per §4.1–§4.3 with zero retraining.
2. First paint ≤ 33 ms from keypress with a warm cache; each subsequent cycle keypress moves the selection within one frame; flick-switch to previous window works at any speed.
3. Windows on other Spaces and screens: listed, previewed, focusable, with correct Space restore behavior.
4. Native switcher restored after `kill -9` of Oriel (verified by test).
5. RSS within §1 targets after 8 h of normal use.
6. Losing Screen Recording permission degrades to Icons style with no crash and no nag loop.

## 9. Open questions

1. **Gestures** — backlog OK, or do you use a swipe trigger today?
2. **Config-first v1** — settings via TOML until M5: acceptable, or is a GUI needed earlier?
3. **macOS 26 minimum** — fine for a personal tool, or should older-OS support be considered?
4. **Peek and drag-drop** — how much do you actually use these? Affects M4 scope.
