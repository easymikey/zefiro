#!/bin/sh
set -eu

binary=$1
shift

if [ "$(basename "$binary")" != zefiro ]; then
    exec "$binary" "$@"
fi

identity=${ZEFIRO_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk '/"Apple Development/ { print $2; exit }')}

if [ -z "$identity" ]; then
    printf 'run-signed: no Apple Development identity and ZEFIRO_SIGN_IDENTITY unset; running %s unsigned\n' "$binary" >&2
    exec "$binary" "$@"
fi

if ! output=$(codesign --force --sign "$identity" --identifier dev.zefiro "$binary" 2>&1); then
    printf 'run-signed: codesign failed for %s\n%s\n' "$binary" "$output" >&2
    exit 1
fi

exec "$binary" "$@"
