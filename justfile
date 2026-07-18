# the pre-commit gate: fmt + clippy (warnings denied) + tests
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# point local git at the committed hooks
hooks:
    git config core.hooksPath .githooks

run:
    cargo run
