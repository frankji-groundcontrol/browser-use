# Off-the-record mode does not work under headless Chromium (negative result)

Date: 2026-08-02 · Scope: Rust port, privacy of the default browser session

## Goal

Make the default session's login "transient like private browsing" — cookies in
memory only, never written to the profile at all. Stronger than the existing
guarantee (throwaway profile, deleted on close).

## What was tried, and why each failed

**1. CDP `Target.createBrowserContext`** (`Browser::start_incognito_context`).
Created successfully, but had no effect: the MCP adopts Chromium's **initial
tab**, which stays in the default browser context. The incognito context existed
and was never used.

**2. The `--incognito` launch flag.** Ignored by headless Chromium.

Both were measured, not assumed. Neither shipped.

## The test that nearly lied

The first test asserted "the cookie value never appears in any file under the
profile" — and **passed with incognito disabled**. Chromium encrypts cookie
values at rest (OS keychain on macOS), so the plaintext is absent either way.

A second attempt compared the profile's file listing; also identical in both
modes, since Chromium scaffolds `Default/Cookies` regardless (20480 bytes, an
empty SQLite allocation).

The discriminator that finally worked is **behavioural**: point both runs at one
fixed profile dir with only the incognito setting varying, then ask whether a
later **on-the-record** session still receives the cookie. It did — in both
mechanisms — which is what settled it.

## Resolution

Reverted; no incognito option is exposed. A privacy switch that does not switch
anything is worse than none.

The transient guarantee that is real and tested: the default profile is deleted
when the session closes, and swept at the next launch if the process was killed
(see [the profile-leak record](2026-08-02-chromium-profile-leak-and-lock-gc.md)).
Cookies do reach disk encrypted *during* a run; they do not survive it.

If never-touching-disk is ever required, the remaining option is a profile on a
RAM-backed filesystem (`tmpfs`, or a macOS RAM disk) — an infrastructure answer,
not a browser flag.

## Lesson

When testing a *negative* ("X is not written / not visible"), first confirm the
test can fail: run it with the feature off. A negative assertion that passes
unconditionally proves nothing, and encryption-at-rest silently makes
"grep for the secret" one of those.
