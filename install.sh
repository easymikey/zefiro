#!/bin/sh
set -eu

base=https://github.com/easymikey/zefiro/releases/latest/download
dir=${ZEFIRO_INSTALL_DIR:-$HOME/.local/bin}

fail() {
    printf 'install: %s\n' "$1" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] || fail 'zefiro runs on macOS only'

case "$(uname -m)" in
    arm64) triple=aarch64-apple-darwin ;;
    x86_64) triple=x86_64-apple-darwin ;;
    *) fail "unsupported architecture $(uname -m)" ;;
esac

archive=zefiro-$triple.tar.gz
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'install: downloading %s\n' "$archive"
curl -fsSL -o "$tmp/$archive" "$base/$archive"
curl -fsSL -o "$tmp/checksums.txt" "$base/checksums.txt"

expected=$(awk -v name="$archive" '$2 == name { print $1 }' "$tmp/checksums.txt")
[ -n "$expected" ] || fail "no checksum for $archive in checksums.txt"
actual=$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive"

tar -xzf "$tmp/$archive" -C "$tmp" zefiro
mkdir -p "$dir"
install -m 755 "$tmp/zefiro" "$dir/zefiro"

printf 'install: zefiro installed to %s/zefiro\n' "$dir"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) printf 'install: warning: %s is not on your PATH; add it to your shell profile\n' "$dir" >&2 ;;
esac
