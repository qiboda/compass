# AGENTS.md — compass

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- No external dependencies yet.

## Commands

```sh
cargo build
cargo run
cargo test
cargo fmt
cargo clippy
```

## Conventions

- Binary crate (`src/main.rs`). `Cargo.lock` must be committed.
- No workspace — single-crate project.
