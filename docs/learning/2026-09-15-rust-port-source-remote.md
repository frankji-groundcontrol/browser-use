# 2026-09-15 — Port source-remote settings at the policy boundary

The smallest faithful port for `BROWSER_USE_DISABLE_SECURITY` is a field on the shared Rust `UrlPolicy`, checked before URL parsing. This keeps every actor caller consistent and avoids per-tool guards. Existing `Browser::connect` supports both HTTP DevTools URLs and raw WebSocket endpoints.
