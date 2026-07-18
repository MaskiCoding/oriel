<img src="assets/icon.svg" width="72" align="right" alt="">

# Oriel

A keyboard-driven window switcher for macOS. Hold a modifier, tap a key, see every window as a live tile, release to jump.

An oriel is a bay window that projects outward so you can see in every direction at once. Same idea: every window from every app, every Space, every screen — one strip, ordered by how recently you touched them.

**Status: pre-alpha.** Nothing usable yet. The full spec lives in [PRD.md](PRD.md).

## What it will be

- **Window-level switching** — the unit is the window, not the app, ordered by global recency of focus.
- **Muscle-memory triggers** — `⌘⇥` for everything, `⌥⇥` scoped to the active app; hold, cycle, release.
- **Instant** — first paint within two frames; a quick flick still switches without engaging the UI.
- **Light and quiet** — menu-bar resident, one process, configured by a TOML file, zero network calls, no telemetry.

## Requirements

- macOS 26+
- Rust (pinned in `rust-toolchain.toml`) and [`just`](https://github.com/casey/just)

## Developing

```sh
just hooks   # once: wire the committed pre-commit hook
just check   # fmt + clippy (warnings denied) + tests — the gate for every commit
just run
```

CI runs the same checks on every PR and push to `main`.

## License

[MIT](LICENSE)
