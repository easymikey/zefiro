#!/bin/sh
# Seeds this worktree's cargo target dir from the coordinator's warm gate build.
# An APFS clone: instant and no extra disk until files diverge.
set -u

target="/tmp/sifr-wt-$(basename "$PWD")"
seed=/tmp/sifr-wt-gate
[ -d "$target" ] || [ ! -d "$seed" ] || /bin/cp -cRp "$seed" "$target"
