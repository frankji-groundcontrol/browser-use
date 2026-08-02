# Browser Use Rust Rewrite

This workspace follows the Rust rewrite plan in
[`../docs/plans/2026-07-05-rust-rewrite/index.md`](../docs/plans/2026-07-05-rust-rewrite/index.md).

## Building

```bash
cargo build            # debug
cargo build --release  # optimized
```

`target/` is gitignored and never committed, but debug incremental artifacts
grow fast (tens of GiB). Reclaim the space with:

```bash
cargo clean
```

The tracked repo stays ~40M regardless — the bulk on disk is always local
build cache, not history.
