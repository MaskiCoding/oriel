<img src="assets/icon.svg" width="72" align="right" alt="">

# Oriel

A keyboard-driven window switcher for macOS. Hold a modifier, tap a key, see every window as a live tile, release to jump.

An oriel is a bay window that projects outward so you can see in every direction at once. Same idea: every window from every app, every Space, every screen — one strip, ordered by how recently you touched them.

**Status: alpha — the loop works.** Hold `⌘` or `⌥`, tap Tab to sweep a strip of app-icon tiles ordered by recency, release to switch. Previews, the config file, and a settings window are still to come. The full spec and milestones live in [PRD.md](PRD.md).

## What works

- **Window-level switching** — the unit is the window, not the app, ordered by recency of focus (yours and Oriel's own switches).
- **Muscle-memory triggers** — `⌘⇥` for everything, `⌥⇥` scoped to the active app; hold, cycle with Tab (`⇧` reverses), release to jump, `⎋` cancels. A quick flick switches to the previous window without engaging the UI.
- **A native strip** — a resident non-activating panel of app-icon + title tiles that wraps to fit the screen; the focused window is raised and de-minimized on switch.
- **Well-mannered** — suppresses the native `⌘⇥` switcher while running and restores it on any exit, including a crash. Menu-bar resident, one process, zero network calls, no telemetry.

## Not yet

Live previews (M2), the TOML config and per-lens filters/styles (M3), app rules and multi-screen (M4), a settings window (M5). See PRD §7.

## Requirements

- macOS 26+
- Rust (pinned in `rust-toolchain.toml`), [`just`](https://github.com/casey/just), and `librsvg` (icon rendering)

## Developing

```sh
just hooks   # once: wire the committed pre-commit hook
just lint    # fmt + clippy (warnings denied) — runs on every commit
just check   # lint + tests — what CI runs on every PR and push to main
just run
```

## License

[MIT](LICENSE)
