#!/bin/sh
set -u

brief="${1:-}"
if [ -z "$brief" ] && [ -f .gate-brief ]; then
    brief=$(cat .gate-brief)
fi

: "${CARGO_TARGET_DIR:=/tmp/sifr-wt-$(basename "$PWD")}"
export CARGO_TARGET_DIR
export RUSTC_WRAPPER=

fail() {
    echo "GATE FAIL: $1"
    printf '%s\n' "$2" | tail -n 40
    exit 1
}

# a. all tracked work committed (untracked files ignored)
output=$(git diff --quiet HEAD 2>&1)
rc=$?
[ "$rc" -eq 0 ] || fail "uncommitted changes" "$output"

# b. at least one commit ahead of main
base=$(git merge-base main HEAD)
head=$(git rev-parse HEAD)
[ "$head" != "$base" ] || fail "no commits ahead of main" "HEAD equals merge-base $base"

# c. house rules over lines added since main
output=$(sh scripts/rules.sh main 2>&1)
rc=$?
[ "$rc" -eq 0 ] || fail "scripts/rules.sh main" "$output"

# d. formatting
output=$(cargo fmt --all --check 2>&1)
rc=$?
[ "$rc" -eq 0 ] || fail "cargo fmt --all --check" "$output"

# e. clippy
output=$(cargo clippy --workspace --all-targets -q -- -D warnings 2>&1)
rc=$?
[ "$rc" -eq 0 ] || fail "cargo clippy --workspace --all-targets" "$output"

# f. tests
output=$(cargo test --workspace -q 2>&1)
rc=$?
[ "$rc" -eq 0 ] || fail "cargo test --workspace -q" "$output"

# g. acceptance, when a brief is known
if [ -n "$brief" ]; then
    output=$(sh scripts/accept.sh "$brief" 2>&1)
    rc=$?
    [ "$rc" -eq 0 ] || fail "scripts/accept.sh $brief" "$output"
fi

git rev-parse HEAD > "$(git rev-parse --git-dir)/gate-ok"
echo "GATE OK $(git rev-parse --short HEAD)"
