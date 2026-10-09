#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

repo=easymikey/zefiro

fail() {
    printf 'release: %s\n' "$1" >&2
    exit 1
}

[ -z "$(git status --porcelain)" ] || fail 'the working tree has changes; commit them first'
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || fail 'not on main'

git fetch -q origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail 'HEAD is not origin/main; push or pull first'

pkgid=$(cargo pkgid -p zefiro)
current="${pkgid##*[#@]}"
version=${1:-$current}
version=${version#v}
tag="v$version"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    fail "tag $tag exists locally"
fi
if [ -n "$(git ls-remote --tags origin "refs/tags/$tag")" ]; then
    fail "tag $tag exists on origin"
fi

if [ "$version" != "$current" ]; then
    sed -i '' '/^\[workspace.package\]/,/^\[/ s/^version = ".*"/version = "'"$version"'"/' Cargo.toml
    cargo metadata --format-version 1 >/dev/null
    git add Cargo.toml Cargo.lock
    git commit -q -m "chore(release): $tag"
fi

git tag -a "$tag" -m "zefiro $version"
git push origin main "$tag"

printf 'release: %s pushed; CI builds it at https://github.com/%s/actions\n' "$tag" "$repo"
