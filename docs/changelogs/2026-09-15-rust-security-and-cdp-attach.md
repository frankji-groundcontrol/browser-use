# 2026-09-15 — Rust security override and DevTools attachment

Ported the upstream `BROWSER_USE_DISABLE_SECURITY` setting into Rust's URL policy. Rust already supports attaching to an existing Chrome DevTools endpoint through `BROWSER_USE_CDP_URL`; `chromiumoxide` resolves HTTP endpoints through `/json/version` and attached sessions are never closed by the Rust process.

Validation: `cargo test --workspace` (all unit and doc tests passed) and
`cargo fmt --all -- --check`. The actor policy regression test now covers the
new field explicitly. The repository image does not include the `pre-commit`
executable, so that hook could not be run here.
