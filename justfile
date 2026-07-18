# fast gate, run by the pre-commit hook: fmt + clippy (warnings denied)
lint:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

# the full gate, run by ci: lint + tests
check: lint
    cargo test --workspace

# point local git at the committed hooks
hooks:
    git config core.hooksPath .githooks

run:
    cargo run
