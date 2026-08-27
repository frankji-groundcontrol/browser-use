# Live-Chrome tests fail intermittently under load

Date: 2026-08-27 · Scope: `bu-actor` and `bu-agent` tests behind `live-chrome`

## Problem

Three tests fail non-deterministically, which repeatedly raises a false alarm
during unrelated work — the natural reading of a red suite is "my change broke
this", and it costs a stash-and-bisect every time to establish otherwise.

| Test | Crate | Symptom |
| --- | --- | --- |
| `a_capture_that_never_completes_leaves_no_stale_indices` | `bu-actor` | fails under full-workspace load, passes with `-p bu-actor` |
| `wedged_command_times_out_and_actor_survives` | `bu-actor` | same |
| `a_failed_capture_still_gives_the_model_a_turn` | `bu-agent` | `browser actor dropped command … channel closed` |

## Evidence that it is pre-existing

Measured by stashing all working changes and running against clean `HEAD`:

- `bu-agent::a_failed_capture_still_gives_the_model_a_turn` — **1 of 3 runs
  failed on clean HEAD**, and 1 of 3 with changes applied. Same rate.
- `bu-actor::a_capture_that_never_completes_leaves_no_stale_indices` — failed on
  clean HEAD with `-p bu-actor` alone, then passed on a re-run.
- Both `bu-actor` tests pass consistently when that crate runs alone and fail
  together when the whole workspace runs serially.

The failing tests are exactly the ones that deliberately induce a *timeout* or a
*wedged command*, so they assert on timing. Under full-suite load — other crates'
Chromium instances still shutting down, CPU contended — the timing budget they
assume no longer holds, and the actor's channel closes before the assertion.

## Impact

Not a product defect: each of these passes in isolation, and the behaviour they
cover works. The cost is diagnostic noise. A red suite that is red for
environmental reasons trains the reader to ignore it, which is how a genuine
regression gets waved through.

## Not fixed here

Left alone deliberately — a timing fix should come from someone who can decide
whether the right answer is a longer budget, a fake clock, or serializing
Chromium across crates. Doing it opportunistically inside an unrelated change
would bury a behavioural decision in a diff about something else.

## Working around it meanwhile

Run the suite excluding the flaky crate, then that crate alone:

```bash
cargo test --workspace --exclude bu-actor --features live-chrome -- --test-threads=1
cargo test -p bu-actor --features live-chrome -- --test-threads=1
```

A failure in these three is only meaningful if it reproduces with the crate run
alone, on a quiet machine.

## Proposed fix

Make the timing explicit rather than ambient: inject the timeout budget so the
tests assert on a controlled clock instead of wall-clock under load, or gate
Chromium-launching tests behind a workspace-wide lock so only one crate drives a
browser at a time. Acceptance: 20 consecutive full-workspace serial runs with
zero failures.
