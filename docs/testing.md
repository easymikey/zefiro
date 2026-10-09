# Testing

How the tests are laid out and run. The rules a test must follow (what is tested where, how a refusal is asserted, hardware tests, snapshots, what agents run) are in `docs/conventions.md` §13; this file does not restate them.

Tests use `rstest` (fixtures and `#[case]` tables) and `insta` (snapshot assertions), with fixtures on disk.

## One integration binary per crate

Each crate with integration tests has one: `tests/main.rs`. The one exception is `macos`, which has only `tests/main_loop.rs`, declared in its `Cargo.toml` with `harness = false` because it must own the main thread. `tests/main.rs` declares the tiers as top-level modules: `mod support;` (shared fixtures, no tests), `mod unit;`, `mod table;` (`rstest` `#[case]` tables of (state, message) rows) and, in `kernel`, `mod compile_fail;` (see below). A test file is a module under its tier directory (`tests/unit/timers.rs`), so its tests are named by tier and module (`unit::timers::…`) and the tier is the filter:

```
cargo test -p kernel --test main unit::timers::    # one module
cargo test -p kernel --test main -- --skip unit::  # everything but one tier
```

One binary per crate means one link per crate instead of one per file; the cost is that one compile error blocks the whole crate's tests, and that `tests/main.rs` must list every tier module. Private functions are tested in `#[cfg(test)] mod tests` at the bottom of their own file, not here.

## The guards crate

Every guard that reads source text lives in `crates/zefiro-guards/tests/guards/`, whichever crate it scans, on one `guards/support.rs`: one directory walk, one `source_files(areas)` list keyed by crate-relative path (`widgets/src/screen/mod.rs`), one `Allow { path, pattern, reason }` row, one `stale` check, one `report`. `cargo test -p zefiro-guards` runs all of them in under a second.

| guard | holds (rule in conventions) |
|---|---|
| `comments.rs` | only the listed `SAFETY:` / `PROTOCOL:` / `GUARD:` one-liners (§9) |
| `builders.rs` | no generated `builder()`, no `maybe_` setter; setters are named after their fields (§2) |
| `conventions.rs` | banned words and loop names from the conventions vocabulary (§9); item rules: `mem::take` of a `self` field inside a `transition` body (§3.5), part names (§3.7), one-field variants (§5.1), no `Result` alias (§5.5), `let _` and `.ok();` discards (§6.3) |
| `config_doc_config.rs`, `config_doc_appearance.rs` | `docs/config.md`'s default blocks still parse into the config defaults |
| `demeter.rs` | kernel `update` handlers take slices, not a whole `Model` (§1.3) |
| `demeter_views.rs` | widgets below `screen` never hold a whole `Model` (§1.3) |
| `dispatch.rs` | `update` has one call site; the paint path never calls it (§1.4) |
| `errors.rs` | error enum shape, `From` along real crate edges and no swallowed write call (§6) |
| `forbidden_names.rs` | verb module files, `get_`, `should_`/`wants_`/`needs_`, mechanism constructors (§9) |
| `hardware.rs` | every test that touches hardware is `#[ignore]` (§13.4) |
| `imports.rs` | every `use` is absolute, no glob, no `pub use` of any visibility (§9); the clippy `pub_use` lint stays off because `bon` builders expand to `pub use` |
| `layering.rs` | the layer map; no widgets module outside `src/screen/` imports `crate::screen` (§1.1, §11.10) |
| `length.rs` | ≤800 lines per file, tests included (§9) |
| `macros.rs` | no `macro_rules!`, no proc-macro crate of our own (§9) |
| `naming.rs` | full words, mechanism names, retired names, parameter names (§9) |
| `public_types.rs` | one public type name lives in one crate (`Error` exempt) |
| `test_files.rs` | no source file exists only for `#[cfg(test)]` (§13) |
| `purity.rs` | `kernel` and `widgets` touch no IO, clock, thread or environment (§1.2) |
| `value_names/` | value names follow the domain words (§8); the baseline `value_names_baseline.txt` is empty |
| `wildcard_arms.rs` | no wildcard arm over an enum of another crate |
| `lexer.rs` | the shared tokenizer of `conventions.rs` and `wildcard_arms.rs` |

`fault.rs` holds the error type the `config_doc` guards fail through. A guard that would repeat a denied clippy lint (`bool` parameters, panics, indexing) does not exist: the lint is the guard; wildcard arms get a guard only where clippy cannot see the enum's crate.

Every other guard refuses every hit; only `comments.rs` and `demeter.rs` keep an allowlist. Those lists are shrink-only and the shrinking is enforced: a row whose `(path, pattern)` no longer matches anything turns its guard red and prints the row's `reason`, so a paid-off debt cannot be re-spent elsewhere. A new exemption is a rule change in `docs/conventions.md`, not an allowlist row.

The `config_doc` guards are the exception that reach into another crate's own types (`config`) to compare `docs/config.md` against a schema instead of scanning source text; `zefiro-guards` carries them as dependencies for that. `zefiro-guards` is not a layer in the layering table: nothing depends on it.

## Hardware tests

Tests needing a real audio device, CoreAudio or an FSEvents watcher carry `#[ignore = "hardware: …"]` and are skipped by default. They live next to the code they drive (`--lib`), not in `tests/`; run them with `cargo test -p runtime -- --include-ignored` (the output device, the macOS driver, the watcher), `-p audio` (its device tests) and `-p macos` (the system volume write). CI does not run them.

## rstest: fixtures and cases

Shared fixtures for a crate's integration tests live in `tests/support/mod.rs` (plus sibling files such as `kernel/tests/support/router.rs`), declared once by `tests/main.rs` and reached from any tier module as `crate::support::…`.

- `#[fixture]` functions build reusable test state (a `Model`, a `Theme`, a `Terminal<TestBackend>`, …). Give parameters `#[default(...)]` values rather than requiring every caller to supply them; reach for `#[with(...)]` overrides or a second fixture instead of a params struct.
- `#[rstest] #[case(a, b)] #[case(c, d)] fn name(#[case] a: T, ...)` runs one test per case with a readable `case_N` name. `#[case]` parameters mix freely with fixture parameters, which are injected by name.
- A machine's table is one row per (state, message) pair, refusals included (§13.3): a refused row checks `Err(Unhandled)` and that the state did not change.

## insta: snapshot assertions

`insta::assert_snapshot!`/`assert_debug_snapshot!` write a `.snap` file on first run, then diff against it on every later run. Commit the `.snap` files: they are the expected output.

- Snapshots live in the test file's own `snapshots/` directory (`tests/unit/snapshots/`, `tests/table/snapshots/`); a file that wants another place says so with `insta::with_settings!({ snapshot_path => "…" }, { … })`. Snapshot file names carry the binary and module path (`main__unit__timers__….snap`).
- `insta::glob!("fixtures/<…>/*.ext", |path| { … })` runs the closure once per matched file and names each snapshot after the file, so a fixture directory gets one snapshot per input with no per-file boilerplate.
- Updating: `cargo insta test --accept` (or `cargo insta review` to approve each diff). Without `cargo-insta`, `INSTA_UPDATE=always cargo test -p <crate>` overwrites the `.snap` files directly; review `git diff` before committing.
- CI runs with `INSTA_UPDATE=no` (`.github/workflows/ci.yml`), so a mismatch fails instead of writing a `.snap.new`.

## compile_fail: programs that must not compile

`crates/kernel/tests/compile_fail/*.rs` are standalone programs `trybuild` compiles and expects to fail, each against a committed `.stderr`. The tier holds only the cases no runtime test can reach: `playlist_jump_rejects_a_library_index` (a `TrackIndex` where a `ViewIndex` belongs would simply play the wrong track) and `cursor_cannot_be_built_out_of_range` (a `Cursor` literal with `index > len`, which proptests cannot claim about the type).

It is its own tier because it costs seconds, not microseconds: each case is a fresh `rustc` over the built crate.

```
cargo test -p kernel --test main compile_fail::                          # just these
cargo test -p kernel --test main -- --skip compile_fail::                # everything else
TRYBUILD=overwrite cargo test -p kernel --test main compile_fail::       # regenerate
```

Regenerating rewrites the `.stderr` files from whatever the current toolchain says; read the diff before committing.

## Fixtures on disk

A real file under `tests/fixtures/` read with `include_str!` or `std::fs::read_to_string` is preferred over fixture content inlined as a string literal: easier to diff, highlight and reuse. Examples: `config/tests/fixtures/*.toml`, `library/tests/fixtures/*.m3u`, `history.jsonl`, `tone.wav`.

## Benchmarks

There are no benchmarks in the tree; performance is measured only on request (§12.9). CI still runs `cargo bench --workspace --no-run`, which compiles any bench a crate adds.

## Lints

Every lint in `[workspace.lints.clippy]` and `[workspace.lints.rust]` is `deny` on every target (`--lib`, `--bins`, `--tests`, `--benches`, `--examples`), and `clippy::allow_attributes` is denied too, so silencing a lint fails the build. Thresholds (function length, parameter count, cognitive complexity, `bool` counts) live in root `clippy.toml`, together with the `allow-*-in-tests` switches; every crate manifest carries `[lints] workspace = true`. CI runs clippy as one step, the same line to run before a commit:

```
cargo clippy --workspace --all-targets -- -D warnings
```
