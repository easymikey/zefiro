#!/bin/sh
# Line-coverage floors: for each workspace crate except zefiro-guards, runs the
# crate's own tests under cargo llvm-cov, then checks the line coverage against
# the crate's floor. Only code no test can reach is left out of the count:
# crates/macos/src/ffi.rs, where every fn calls into the MediaPlayer framework.
# Prints COVERAGE OK when every crate meets its floor. Otherwise it prints
# COVERAGE FAIL <crate>: tests failed, COVERAGE FAIL <crate> under its floor of
# <floor>% (with the measured figure), COVERAGE FAIL <crate>: report failed or
# COVERAGE FAIL <crate> has no floor for each failing crate, and exits 1.
# Runs from any directory. Floors are the measured line coverage rounded down,
# minus 2. Before the crate loop it cleans the workspace's coverage build once:
# objects from an earlier run made a second run measure each crate several
# points low.
set -u

cd "$(dirname "$0")/.." || exit 1

unreachable='crates/macos/src/ffi\.rs'

floors='
audio 79
config 95
kernel 95
library 92
macos 68
runtime 91
terminal 78
widgets 95
zefiro 69
'

: "${CARGO_TARGET_DIR:=target/llvm-cov}"
export CARGO_TARGET_DIR
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}"

cargo bin cargo-llvm-cov clean --workspace --color never >/dev/null 2>&1

failed=''
for manifest in crates/*/Cargo.toml; do
    crate=$(awk -F'"' '/^name = / { print $2; exit }' "$manifest")
    if [ "$crate" = "zefiro-guards" ]; then
        continue
    fi
    floor=$(printf '%s\n' "$floors" | awk -v crate="$crate" '$1 == crate { print $2 }')
    if [ -z "$floor" ]; then
        echo "COVERAGE FAIL $crate has no floor"
        failed="$failed $crate"
        continue
    fi

    cargo bin cargo-llvm-cov clean --profraw-only --color never >/dev/null 2>&1
    if ! output=$(cargo bin cargo-llvm-cov --no-report -p "$crate" --color never 2>&1); then
        printf '%s\n' "$output" | tail -n 15
        echo "COVERAGE FAIL $crate: tests failed"
        failed="$failed $crate"
        continue
    fi

    if report=$(cargo bin cargo-llvm-cov report -p "$crate" --summary-only --color never \
        --ignore-filename-regex "$unreachable" --fail-under-lines "$floor" 2>&1); then
        echo "coverage $crate $(printf '%s\n' "$report" | awk '$1 == "TOTAL" { print $10 }') (floor $floor%)"
        continue
    fi
    measured=$(printf '%s\n' "$report" | awk '$1 == "TOTAL" { print $10 }')
    if [ -z "$measured" ]; then
        printf '%s\n' "$report" | tail -n 15
        echo "COVERAGE FAIL $crate: report failed"
    else
        echo "COVERAGE FAIL $crate under its floor of $floor%: measured $measured"
    fi
    failed="$failed $crate"
done

if [ -n "$failed" ]; then
    echo "COVERAGE FAIL:$failed"
    exit 1
fi
echo "COVERAGE OK"
