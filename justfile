# fast gate, run by the pre-commit hook: fmt + clippy (warnings denied)
lint:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

# the full gate: lint + tests + the release compile. Debug-only code is
# #[cfg]-gated, so only a release build catches a stray call into it.
check: lint
    cargo test --workspace
    cargo build --release

# point local git at the committed hooks
hooks:
    git config core.hooksPath .githooks

# build + sign the app bundle, then launch it with stdout attached
run:
    ./scripts/mkbundle.sh debug
    ./dist/Oriel.app/Contents/MacOS/oriel

# release build into /Applications
install:
    ./scripts/mkbundle.sh release
    rm -rf /Applications/Oriel.app
    ditto dist/Oriel.app /Applications/Oriel.app

# render iconset + menu-bar template into dist/ from the committed svg masters
icons:
    ./scripts/mkicons.sh dist
