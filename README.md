<img src="assets/icon.svg" width="72" align="right" alt="">

# Oriel

A keyboard-driven window switcher for macOS. Hold a modifier, tap a key, see every window as a live tile, release to jump.

An oriel is a bay window that projects outward so you can see in every direction at once. Same idea: every window from every app, every Space, every screen — one strip, ordered by how recently you touched them.

## What it does

- **Window-level switching.** The unit is the *window*, not the app. Every window from every app, Space, and screen, in one strip, ordered by global recency of focus — yours as well as Oriel's own switches.
- **Muscle-memory triggers.** `⌘⇥` for everything, `⌥⇥` scoped to the active app. Hold, cycle with the trigger key (`⇧` reverses), release to jump; `⏎` focuses, `⎋` cancels. A quick flick switches to the previous window without the strip ever appearing.
- **Lenses.** A lens is a trigger plus a scope plus a look. Each one picks its own windows (all apps / active only / everything else; all Spaces / visible / hidden; all screens / the strip's screen), decides what to do with minimized, hidden, fullscreen, and windowless entries (show, show at the end, or hide), and chooses its ordering, grouping, style, and size. Hitting another lens's trigger mid-session morphs the strip in place.
- **Live previews.** Tiles are real window screenshots, including minimized and off-Space windows. The strip always paints from cache first, so summoning never waits on a capture.
- **Three presentations.** Gallery (preview tiles, the default), Icons (a compact icon grid), List (dense rows). Tile size is fixed or automatic — with many windows the strip densifies rather than scrolling. It never scrolls.
- **Filter.** Type to narrow: exact, prefix, word-boundary, substring, acronym, then small-edit fuzzy, diacritic-insensitive, app name weighted above window title, with matched characters highlighted. Matches sort above non-matches instead of vanishing, so the layout does not jump while you type.
- **In-session actions.** `W` closes a window, `M` minimizes or restores, `F` toggles fullscreen, `Q` quits the app, `H` hides it. Arrows move the selection in two dimensions across the strip's real rows and wrap at the edges; vim keys optional. All rebindable.
- **Mouse, without stealing focus.** Click a tile to jump, scroll to move the selection, and optionally hover to select. The strip is a non-activating panel throughout — pointing at it never deactivates the app underneath.
- **Peek.** Off by default: when on, the selected window is also shown at full size behind the strip, so you can read it before committing.
- **Captions your way.** Show the window title, the app name, or both; truncate at the start, middle, or end; hide the state markers and Space badges entirely.
- **App rules.** Keyed by bundle-ID prefix: hide an app's windows (always, when it has no window, or by title), or pass the trigger straight through to the app — which is what makes VMs and remote-desktop clients usable.
- **Well-mannered.** Suppresses the native `⌘⇥` switcher while running and restores it on every exit path, including `kill -9`. Menu-bar resident with no Dock icon — it becomes a regular app only while the settings window is open, so that window reaches the Dock and `⌘⇥` like any other. One process. **Zero network calls, no telemetry, no accounts, no auto-updater.**
- **Nothing animates in.** The strip appears instantly. Dismissal can fade if you ask for it; nothing else moves.

## Configuration

The source of truth is a TOML file at `~/.config/oriel/config.toml`, written with the defaults on first launch and hot-reloaded when you save it. A file that fails to parse is reported and ignored — the running app keeps the last good config rather than falling over while you edit.

The settings window is a view over that same file: anything it changes, it writes there, and the watcher picks it up. It rewrites the whole file, so comments you add by hand do not survive a save from the window — edit the TOML directly if you want to keep them.

```toml
summon_delay_ms = 0            # 0–900; 0 means the strip appears immediately
theme = "system"               # light | dark | system
show_on = "active-screen"      # active-screen | pointer-screen | menubar-screen
background_capture = true      # keep previews warm; off captures only during a session
start_at_login = true
menubar_icon = true

[peek]
enabled = false

[animation]
fade_out = false               # dismissal only; nothing ever animates in

[titles]
show = "title"                 # title | app | both
truncate = "end"               # start | middle | end
markers = true                 # state markers (● ⤢) and Space badges

[controls]
arrow_keys = true
vim_keys = false
hover_select = false
cursor_follows_focus = "never" # never | always | other-screen

[keys]                         # in-session actions, single keys
focus = "return"
cancel = "escape"
close = "w"
minimize = "m"
fullscreen = "f"
quit_app = "q"
hide_app = "h"

[[lens]]
trigger = "cmd+tab"
apps = "all"                   # all | active | inactive
spaces = "all"                 # all | visible | hidden
screens = "all"                # all | strip-screen
minimized = "show"             # show | end | hide
hidden = "show"
fullscreen_windows = "show"
windowless_apps = "end"
order = "recent"               # recent | created | alphabetical | space
group_by_app = false
style = "gallery"              # gallery | icons | list
size = "auto"                  # small | medium | large | auto
on_release = "jump"            # jump | linger | filter

[[lens]]
trigger = "alt+tab"
apps = "active"                # unset keys inherit the first lens

[[rule]]                       # bundle-ID prefix match
bundle_prefix = "com.apple.finder"
hide_windows = "windowless"    # never | always | windowless | title-contains

[[rule]]
bundle_prefix = "com.parallels."
pass_triggers = "fullscreen"   # never | always | fullscreen
```

That is the whole schema; the file written on first run contains all of it. Unknown keys are
rejected rather than ignored, so a typo is reported instead of silently doing nothing.

## Requirements

macOS 26 or newer. Accessibility permission is required; Screen Recording is optional — without it Oriel runs in Icons style with previews and window titles disabled, and says so once.

Oriel uses private system APIs to see and focus windows across Spaces, which is why it is not on the App Store. Every private symbol is resolved at runtime and degrades to a reduced capability rather than crashing if a macOS release removes it.

## Building

```sh
just hooks     # once per clone: point git at the committed pre-commit hook
just check     # fmt + clippy (warnings denied) + tests — the merge gate
just run       # build, sign, and launch the app bundle
just install   # release build into /Applications
```

Rust toolchain is pinned in `rust-toolchain.toml`; icon rendering needs `librsvg`.

## Layout

A Cargo workspace, one concern per crate, with the unsafe FFI quarantined at the bottom:

| crate | concern |
|---|---|
| `skylight-sys` | `extern "C"` declarations for private SkyLight/CGS symbols. Zero logic. |
| `ax` | Safe wrapper over the C Accessibility API. |
| `model` | Pure state: windows, MRU, and the filter/order/group/match resolvers. No FFI, no AppKit, fully unit-tested. |
| `winsrv` | WindowServer enumeration, batched state queries, Space topology, the focus sequence. |
| `capture` | Window screenshots, thumbnailing, and a byte-budgeted preview cache. |
| `input` | Triggers, native-switcher suppression, event taps, keymap parsing. |
| `ui` | The strip, Peek, and the settings window as native AppKit via `objc2`. |
| `config` | TOML schema, defaults, and serialisation back to disk. |
| `src/` | Thin orchestration: the session loop, the menu bar, lifecycle. |

## License

[MIT](LICENSE)
