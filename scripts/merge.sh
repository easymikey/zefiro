#!/bin/sh
set -eu
export CARGO_BUILD_JOBS=8

slot="${1:?usage: merge.sh <slot-N>}"
tree=".work/worktrees/$slot"
log=/tmp/t.log

if git log --format=%B "main..$slot" | rg -i 'co-authored'; then
    printf 'refused: %s has attribution in commit messages\n' "$slot"
    exit 1
fi

commits=$(git rev-list --reverse "main..$slot")
if [ -n "$commits" ]; then
    if ! git cherry-pick $commits; then
        git status --short
        printf 'conflict while picking %s\n' "$slot"
        exit 1
    fi
fi

cargo clippy --workspace --all-targets -q -- -D warnings

FULL_CHECK=1 cargo test --workspace --no-fail-fast -q --message-format=short > "$log" 2>&1 || true
if rg '^\S+ --- FAILED|signal:|\.snap\.new|[1-9]\d* stale|^error' "$log"; then
    printf 'tests failed, see %s\n' "$log"
    exit 1
fi

git -C "$tree" switch -q -C "$slot" main
printf 'MERGED %s\n' "$(git rev-parse --short HEAD)"
