#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

tap=local/zefiro
target=aarch64-apple-darwin

if [ "$(uname -sm)" != "Darwin arm64" ]; then
    printf 'brew-tap: builds only on an Apple silicon Mac\n' >&2
    exit 1
fi

identity=${ZEFIRO_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk '/"Apple Development/ { print $2; exit }')}

if [ -z "$identity" ]; then
    printf 'brew-tap: no Apple Development identity and ZEFIRO_SIGN_IDENTITY unset; an unsigned build makes the Keychain refuse the saved server password after every upgrade\n' >&2
    exit 1
fi

cargo build --release --locked -p zefiro

pkgid=$(cargo pkgid -p zefiro)
version="${pkgid##*[#@]}.$(git log -1 --format=%cd --date=format:%Y%m%d%H%M)"

dist=target/brew
stage=$dist/stage
archive=$dist/zefiro-$version-$target-$(date +%Y%m%d%H%M%S).tar.gz

mkdir -p "$stage"
cp target/release/zefiro "$stage/zefiro"
codesign --force --sign "$identity" --identifier dev.zefiro "$stage/zefiro"
tar -czf "$archive" -C "$stage" zefiro

sha=$(shasum -a 256 "$archive" | awk '{ print $1 }')
formula_dir="$(brew --repository)/Library/Taps/local/homebrew-zefiro/Formula"
mkdir -p "$formula_dir"

cat > "$formula_dir/zefiro.rb" <<EOF
class Zefiro < Formula
  desc "Terminal music player"
  homepage "file://$PWD"
  url "file://$PWD/$archive"
  version "$version"
  sha256 "$sha"
  license "MIT"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "zefiro"
  end

  test do
    assert_match "terminal music player", shell_output("#{bin}/zefiro --help")
  end
end
EOF

installed=$(brew list --versions "$tap/zefiro" 2>/dev/null || true)

case "$installed" in
    "") HOMEBREW_NO_AUTO_UPDATE=1 brew install "$tap/zefiro" ;;
    *" $version") HOMEBREW_NO_AUTO_UPDATE=1 brew reinstall "$tap/zefiro" ;;
    *) HOMEBREW_NO_AUTO_UPDATE=1 brew upgrade "$tap/zefiro" ;;
esac

printf 'brew-tap: zefiro %s installed from %s\n' "$version" "$tap"
