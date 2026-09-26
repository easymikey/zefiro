set shell := ["sh", "-c"]
set positional-arguments

export RUSTC_WRAPPER := ""

# fmt, clippy and tests
default: gate

# fresh clone: pinned tools, cargo aliases, git hooks
setup:
    cargo install cargo-run-bin --locked
    cargo bin --install
    cargo bin --sync-aliases
    cog install-hook --all --overwrite

# versions of everything the recipes rely on
doctor:
    rustup show active-toolchain
    cargo --version
    cargo nextest --version
    cargo llvm-cov --version
    cog --version

# check one crate or the workspace
check crate="":
    @if [ -n "{{crate}}" ]; then cargo check -p {{crate}} --all-targets; else cargo check --workspace --all-targets; fi

# clippy for one crate or the workspace
clippy crate="":
    @if [ -n "{{crate}}" ]; then cargo clippy -p {{crate}} --all-targets -- -D warnings; else cargo clippy --workspace --all-targets -- -D warnings; fi

# tests, filter optional: just test -p kernel, just test router
test *filter:
    cargo nextest run {{filter}}

# house rules clippy cannot see, over lines added since a base
rules base="main":
    sh scripts/rules.sh {{base}}

# fmt, clippy, tests, house rules, acceptance: brief defaults to .gate-brief
gate brief="":
    sh scripts/gate.sh {{brief}}

# acceptance block from a brief: just accept docs/briefs/foo.md
accept brief:
    sh scripts/accept.sh {{brief}}

# accept pending insta snapshots after reading the diffs
snap-accept *filter:
    INSTA_UPDATE=always cargo nextest run {{filter}}

# coverage html, opens the browser
coverage:
    cargo llvm-cov nextest --workspace --html --open

# conventional commit: just commit feat kernel "what changed"
commit type scope message:
    cog commit {{type}} "{{message}}" {{scope}}

# bump the version from the history and tag it vX.Y.Z
bump level="auto":
    cog bump --{{level}}

# build output, rust-analyzer target, coverage
clean:
    cargo clean
    rm -rf target/rust-analyzer target/llvm-cov

# size of everything safe to delete
disk:
    @du -sh target ~/.cache/sifr 2>/dev/null; true

# pre-commit hook: fix formatting of fully staged files, then check what is staged, clippy if rust changed
pre-commit:
    #!/bin/sh
    set -e
    staged() { git diff --cached --name-only --diff-filter=ACMR -- "$@"; }
    whole() { for file in "$@"; do git diff --quiet -- "$file" && echo "$file"; done; }
    rs=$(staged '*.rs')
    toml=$(staged '*.toml')
    text=$(staged '*.rs' '*.toml' '*.md' '*.yml' '*.yaml' '*.json' 'justfile')
    fix_rs=$(whole $rs)
    fix_toml=$(whole $toml)
    [ -z "$fix_rs" ] || { rustfmt --edition 2024 $fix_rs; git add $fix_rs; }
    for manifest in $fix_toml; do
        case "$manifest" in */Cargo.toml|Cargo.toml) cargo sort --grouped --no-format "$(dirname "$manifest")" >/dev/null ;; esac
    done
    [ -z "$fix_toml" ] || { taplo fmt $fix_toml 2>/dev/null; git add $fix_toml; }
    [ -z "$rs" ] || rustfmt --check --edition 2024 $rs
    [ -z "$toml" ] || { cargo sort --workspace --grouped --no-format --check; taplo fmt --check $toml; }
    [ -z "$text" ] || typos $text
    [ -z "$rs$toml" ] || cargo clippy --workspace --all-targets -- -D warnings

# pre-push hook: the ci test profile over the workspace
pre-push:
    cargo nextest run --workspace --profile ci
