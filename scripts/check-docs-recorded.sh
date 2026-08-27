#!/bin/sh
# Docs-recording guardrail. Two jobs:
#
#   1. Warn (never block) when a change touches source/config but records nothing
#      under docs/changelogs/.   [default]
#   2. Checkpoint gate: when the agent declares a commit a checkpoint, REQUIRE the
#      handoff trio to be staged and BLOCK (exit 1) if any is missing.   [--checkpoint]
#
# Usage: check-docs-recorded.sh [--staged|--worktree] [--json] [--checkpoint]
#   --staged      inspect staged files       (git pre-commit hook)   [default]
#   --worktree    inspect modified+untracked  (Claude Stop hook / ad-hoc)
#   --json        on a warn trigger, emit {"systemMessage":"..."} on stdout
#                 (for the Claude Stop hook, whose plain stderr on exit 0 is
#                 invisible). Warn mode only; never emits continue:false.
#   --checkpoint  require the handoff trio (a docs/changelogs/ entry, an active
#                 docs/plans/ file, and docs/HANDOFF.md) to be staged; exit 1 if
#                 not. Used by the commit-msg hook for [checkpoint] commits.
#
# Warn mode ALWAYS exits 0. Checkpoint mode exits 1 when incomplete (blocking).
# Bypass any git hook with `git commit --no-verify`. Record dir overridable with
# DOCS_GUARDRAIL_RECORD_DIR. One source of truth for every trigger.
set -u

scope="--staged"
json=0
checkpoint=0
for a in "$@"; do
  case "$a" in
    --staged)     scope="--staged" ;;
    --worktree)   scope="--worktree" ;;
    --json)       json=1 ;;
    --checkpoint) checkpoint=1 ;;
    *) echo "usage: check-docs-recorded.sh [--staged|--worktree] [--json] [--checkpoint]" >&2; exit 0 ;;
  esac
done

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
cd "$root" || exit 0
record_dir="${DOCS_GUARDRAIL_RECORD_DIR:-docs/changelogs}"
handoff="${DOCS_GUARDRAIL_HANDOFF:-docs/HANDOFF.md}"
plans_dir="${DOCS_GUARDRAIL_PLANS_DIR:-docs/plans}"

staged=$(git diff --cached --name-only 2>/dev/null)

# ---- Checkpoint gate (blocking) -------------------------------------------
if [ "$checkpoint" -eq 1 ]; then
  has_changelog=0 has_plan=0 has_handoff=0
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    case "$f" in
      "$record_dir"/README.md) : ;;
      "$record_dir"/*) has_changelog=1 ;;
    esac
    case "$f" in
      "$plans_dir"/README.md) : ;;
      "$plans_dir"/*) has_plan=1 ;;
    esac
    [ "$f" = "$handoff" ] && has_handoff=1
  done <<EOF
$staged
EOF
  missing=""
  [ "$has_changelog" -eq 1 ] || missing="$missing\n  - a dated $record_dir/ entry"
  [ "$has_plan" -eq 1 ]      || missing="$missing\n  - the active $plans_dir/ file"
  [ "$has_handoff" -eq 1 ]   || missing="$missing\n  - $handoff (agent handoff)"
  [ -z "$missing" ] && exit 0
  printf '\n[checkpoint blocked] this commit is tagged a checkpoint but is missing:' >&2
  printf '%b\n' "$missing" >&2
  printf 'Stage those (they let another agent take over), or drop the checkpoint tag,\n' >&2
  printf 'or bypass with: git commit --no-verify\n\n' >&2
  exit 1
fi

# ---- Warn mode (non-blocking) ---------------------------------------------
case "$scope" in
  --staged)   files="$staged" ;;
  --worktree) files=$(git status --porcelain 2>/dev/null | sed 's/^...//') ;;
esac
[ -z "$files" ] && exit 0

source_changed=0
record_changed=0
while IFS= read -r f; do
  [ -z "$f" ] && continue
  case "$f" in
    "$record_dir"/*) record_changed=1 ;;
  esac
  case "$f" in
    docs/*|.gitignore|.DS_Store|*.lock|*/*.lock|*package-lock.json|*pnpm-lock.yaml) : ;;
    *) source_changed=1 ;;
  esac
done <<EOF
$files
EOF

[ "$source_changed" -eq 1 ] && [ "$record_changed" -eq 0 ] || exit 0

if [ "$json" -eq 1 ]; then
  printf '{"systemMessage":"docs guardrail: source changed but no %s/ entry recorded \\u2014 add %s/YYYY-MM-DD-<title>.md and update the nearest index before committing (warn-only)."}\n' \
    "$record_dir" "$record_dir"
else
  printf '\n[docs guardrail] source changed but no %s/ entry is staged/recorded.\n' "$record_dir" >&2
  printf '  Record it: add %s/YYYY-MM-DD-<title>.md and update the nearest index,\n' "$record_dir" >&2
  printf '  then refresh any affected docs/ (architecture, usage, ...).\n' >&2
  printf '  For a milestone another agent takes over from, make it a checkpoint\n' >&2
  printf '  (tag the message [checkpoint] + update %s).\n' "$handoff" >&2
  printf '  warn-only: this does NOT block. Rationale: AGENTS.md repo-organization contract.\n\n' >&2
fi
exit 0
