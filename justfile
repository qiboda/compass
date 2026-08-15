# Compass development command set (see AGENTS.md "Commands" for the source
# of truth). Run `just` to launch the GUI, `just --list` to see all recipes.

run:
    cargo run --bin compass

build:
    cargo build

test:
    cargo test

fmt:
    cargo fmt

clippy:
    cargo clippy -- -D warnings

check:
    cargo fmt -- --check
    cargo clippy -- -D warnings
    cargo test

import:
    cargo run --bin compass-data -- import

export:
    cargo run --bin compass-data -- export

backup:
    cargo run --bin compass-data -- backup
