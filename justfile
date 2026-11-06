build:
    cargo build --workspace

test:
    cargo test --workspace

lint:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check

check: lint test

run-demo:
    cargo run -p spl-app -- --demo

docs:
    cargo doc --workspace --no-deps --open
