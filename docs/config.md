# Configuration

sifr reads two independent TOML files, each hot-reloaded on its own
schedule:

| File | Loaded by | Purpose |
|---|---|---|
| `config.toml` | `sifr_cli::config::Config` (`crates/sifr-cli/src/config.rs`) | music library path, theme selection, volume, audio behavior, media watching, key rebinds |
| `sifr-ui.toml` | `sifr_render::appearance::AppearanceConfig` (`crates/sifr-render/src/appearance.rs`) | every theme-styled widget's geometry/behavior (cover, card, progress), layout breakpoints, and whole-window color repaint |

General rules that apply to both files:

- **Unknown keys are a parse error.** Both `Config` and `AppearanceConfig`
  (and every nested table, and a theme's own `[colors]`) set
  `#[serde(deny_unknown_fields)]`, so a typo or a key from another sifr
  version is reported rather than silently skipped.
- **Both files hot-reload, but not identically.** `sifr-ui.toml` is
  polled by `ConfigWatch` (`WatchedFile::Appearance`) roughly every 500ms (mtime-gated) and *every*
  field in it takes effect on the next poll, no restart needed. In
  `config.toml`, only `[keymap]` and the top-level `music_dir` take effect
  live: the `ConfigWatch` driver watches that file too and sends its raw
  text as `ConfigChange::Keymap`, which `hot_reload::keymap_changed` re-parses
  through `keymap_file::parse_keymap`; `theme`,
  `volume`, `[audio]`, and `media.watch` are read once at startup by
  `Config::load()`. The settings overlay can still change those fields
  live in the running `Model` and persist the new value back to
  `config.toml`, but an *external* edit to one of them (hand-editing the
  file while sifr is running) needs a restart to be picked up.
- **`[keymap]` lives in `config.toml` alone.** An action left unset there
  keeps its built-in chord (see `sifr_core::update::keymap::table` for the
  full action -> default-chord list). An entry may also name the context it
  binds in — see [Key contexts](#key-contexts).
- **`,` (the `settings` action, default chord `,`) opens the settings
  overlay, and can write to both files.** Each row it edits maps to
  either a `ConfigPatch` (persisted to `config.toml` — crossfade,
  replaygain, media watch, theme, sleep presets) or a
  `AppearancePatch` (persisted to `sifr-ui.toml` — cover mode/brackets,
  card format chips/speed chip, progress mode/remaining,
  key hints, animations, modal shadows, key
  buttons, layout mode).
  Both patch writers
  (`sifr-runtime`'s `ConfigDriver`) touch only the field(s) actually
  changed via `toml_edit`, leaving every other key, comment, blank line,
  and table order in the file untouched.
- **`[cover] `mode` alone decides the cover's pixel-vs-text path** —
  there is no separate `[cover] mode` key (unlike `[progress]`,
  which has its own). `sifr-cli`'s
  `resolve_cover_plan` renders `mode = "vinyl"`/`"plain"` on the pixel
  path, dropping the cover column entirely when the terminal has no real
  graphics protocol; `"milkdrop"` (drawn by
  `components::organisms::milkdrop_text`, never from an image) always
  renders without a graphics protocol, and `"off"` draws no cover at all.
- **The `noir` preset is a bundle, not a single key.** Picking `Preset ->
  Noir` in the settings overlay (`sifr_core::domain::appearance::preset_options(AppearancePreset::Noir)`) sets every one of: `[card] format_chips
  = true`, `[cover] `mode` = "milkdrop"`, `[cover] brackets = true`,
  `[progress] mode = "text"`, `[progress] remaining = true` — plus
  selecting the `"noir"` theme — in one shot. You can also hand-assemble
  the same look field-by-field in `sifr-ui.toml`; the preset is just a
  named shortcut for it.

The default blocks below (`<!-- defaults:config -->` / `<!-- defaults:window
-->`) are locked to the real code by
`crates/sifr-cli/tests/config_doc_test.rs` and
`crates/sifr-render/tests/config_doc_test.rs`: each test parses the exact
text between its markers as `Config` / `AppearanceConfig` and asserts the result
equals `Config::default()` / `AppearanceConfig::default()`. If a struct's
`Default` impl changes and this file isn't updated to match, the test
fails — the failure message tells you which block drifted and to update
it by hand (see **Regenerating the default blocks** below).

Neither `Config` nor `AppearanceConfig` derives `serde::Serialize` (both are
`Deserialize`-only — parsing is all either one is used for), so the
blocks below can't be produced by a `toml::to_string_pretty(&T::default())`
call the way a `Serialize`-able config would be. The round-trip
(doc text -> parse -> `assert_eq!(_, T::default())`) gives the same
anti-drift guarantee without it: any value below that doesn't match the
code's real default fails the test.

## `config.toml`

Path: `$XDG_CONFIG_HOME/sifr/config.toml` (`dirs::config_dir().join("sifr").join("config.toml")`;
on macOS that's `~/Library/Application Support/sifr/config.toml`). Missing
file or missing directory both fall back to `Config::default()` silently;
an existing-but-unparsable file is a startup error naming the path.

The scanned library is cached outside that directory, under
`$XDG_CACHE_HOME/sifr/` (`dirs::cache_dir().join("sifr")`; on macOS that's
`~/Library/Caches/sifr/`): `library.bin` holds the tagged tracks and
`library.dir` the `music_dir` they were scanned from. Neither is config — a
missing, stale or unreadable pair just means a rescan.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `music_dir` | path | OS audio directory (`dirs::audio_dir()`), or `.` if that can't be resolved | Root folder scanned for tracks. A leading `~` or `$VAR`/`${VAR}` is expanded against the real home directory/environment (e.g. `~/Music`, `$HOME/Music`). Reloads: the library is rescanned when the path changes (Q109b), no restart needed. Changeable from inside the app too, via the source-folder overlay (default chord `L`), which writes this key verbatim (no expansion) and lets the reload above pick it up. |
| `theme` | string | `"auto"` | Active color theme name. Embedded names (`crates/sifr-render/src/theme/embedded.rs`, in the order the settings overlay's `Theme` row cycles them): `ristretto`, `ember`, `hacker`, `winamp`, `oreo`, `macaroon`, `wafer`, `noir`, `neobrutalism-dark`, `neobrutalism-light`, `terracotta-dark`, `terracotta-light`, `rose-pine`, `rose-pine-dawn`, `gruvbox`, `gruvbox-light`; any `.toml` file in the user themes dir is offered alongside them and a user file shadows an embedded theme of the same name. Read once at startup; the actual theme file is watched for live changes, but the name must be changed via the settings overlay or config reload to take effect. |
| `volume` | integer (`u8`) | `50` (read from `Transport::default().volume` in `sifr-core`, the one place the number lives) | Startup output volume, as a percentage. The field itself is an unchecked `u8`, so anything up to `255` parses; only the `--volume` flag is range-checked (clamped to `0..=100`), and a hand-written value above `100` is clamped by `Percent::clamped` on its way into the `Model` rather than rejected here. Read once at startup; the settings overlay can change it live and persist the new value. |
| `audio.crossfade` | string duration (`"Ns"` or `"Nms"`) | `"0s"` | Crossfade length between tracks; `"0s"` is gapless. It applies to the natural end of a track alone: the next one is preloaded on its own sink and the two fade equal-power over these last seconds. An explicit skip — `n`/`p`, a playlist jump, a browse pick — does not crossfade; `start` emits an `AudioCmd::Stop` before the load for `StartOrigin::User` and nothing for `StartOrigin::TrackEnded` (`crates/sifr-core/src/update/player/mod.rs`), so a skip cuts and only a track running out fades. Parsed into a `Duration` once, at startup; the settings overlay can change it live and persist the new value. |
| `audio.replaygain` | bool | `false` | Apply ReplayGain-based volume normalization. Read once at startup; the settings overlay can change it live and persist the new value. |
| `audio.device` | optional string | unset (`None`) — system default output | Exact device name as listed in the settings overlay's "Output device" row. An unknown name falls back to the system default with a toast. Read once at startup; the settings overlay can change it live and persist the new value. |
| `audio.sleep_presets` | array of whole minutes | `[15, 30, 60]` | Sleep-timer durations cycled by repeated presses of the sleep-timer action (default chord `z`). Also adjustable in the settings overlay's "Sleep presets" row, which cycles a fixed set of bundles (`[15, 30, 60]`, `[10, 20, 45]`, `[30, 60, 90]`, `[45, 90, 120]`, or off); a value hand-edited here that matches none of them snaps to the closest bundle by total minutes on the row's first `h`/`l` press. Read once at startup. |
| `media.watch` | bool | `false` | Watch `music_dir` for filesystem changes via `notify` and auto-rescan (off by default — an extra background thread + OS watch handles most setups don't need, since rescan (`R`) is manual anyway). Read once at startup; no watcher monitors the `[media]` table for changes while sifr runs. |
| `keymap.<action>` | optional chord string, or a table of `chord` + `context` | unset (`None`) for every action | Per-action key rebind, e.g. `play_pause = "space"` or `play_pause = { chord = "space", context = "search" }` (see [Key contexts](#key-contexts)). Unset actions keep their built-in chord. `enqueue` toggles: on a track already in the queue it removes that entry, on any other track it appends one. The 44 actions (`TomlKeymap`'s fields) — `play_pause`, `next`, `prev`, `seek_back`, `seek_forward`, `seek_back_short`, `seek_forward_short`, `seek_back_long`, `seek_forward_long`, `volume_up`, `volume_down`, `shuffle`, `repeat`, `down`, `up`, `top`, `bottom`, `page_down`, `page_up`, `play_selected`, `enqueue`, `play_next`, `dequeue`, `queue_move_up`, `queue_move_down`, `cycle_sort`, `favorite`, `delete`, `save_playlist`, `rescan`, `search`, `history`, `settings`, `help`, `quit`, `sleep_timer`, `ab_repeat`, `speed_down`, `speed_up`, `toggle_key_hints`, `cycle_layout`, `jump_to_time`, `track_details`, `source_dir` — see `crates/sifr-core/src/update/keymap/table.rs` for every built-in chord and `Chord::parse` for accepted chord spellings (bad syntax parses fine as a string; it only surfaces as a validation error later). Hot-reloaded by the `ConfigWatch` driver roughly every 500ms when edited externally or changed via the settings overlay. |

`music_dir` is left out of the default block below since its default is
computed from the OS at runtime (`dirs::audio_dir()`), not a fixed
literal — the doc test only locks down the fields that have one.

<!-- defaults:config -->
```toml
theme = "auto"
volume = 50

[audio]
crossfade = "0s"
replaygain = false
# device = "MacBook Pro Speakers"
sleep_presets = [15, 30, 60]

[keymap]
# no overrides by default — every action uses its built-in chord
```
<!-- /defaults:config -->

### Key contexts

A `[keymap]` entry takes two spellings. The short one is a chord string and
binds the action in the `global` context:

```toml
[keymap]
next = "N"
```

The long one is a table that also names the context the binding applies in:

```toml
[keymap]
next = { chord = "ctrl+n", context = "search" }
```

The contexts, and what is in focus in each:

| Context | In focus |
|---|---|
| `global` | the whole app — playback, the overlays' open chords, quit |
| `playlist` | the browsing list: selection, queue edits, sort, rescan |
| `search` | the search overlay's query line and result list |
| `help` | the help overlay |
| `history` | the history overlay's list |
| `settings` | the settings overlay's rows |
| `confirm_delete` | the delete confirmation |
| `jump_to_time` | the jump-to-time prompt |
| `track_details` | the track info overlay |
| `text_prompt` | the save-playlist and source-folder prompts |

**Inside an overlay only that overlay's bindings apply.** An overlay is
modal: while one is open its context is the whole stack, so a `global` or
`playlist` binding does not reach through it — that is the key-context rule in
`docs/conventions.md` §3.8. A binding that names an
overlay's context overrides that overlay's own row for the same chord, on
the same terms a `global` entry overrides a built-in global chord.

An unknown context, an unknown field, or a table without a `chord` is a
parse error for the whole file, like any other malformed key.

## Theme files (`*.toml`)

Path: any `.toml` in the user themes dir, or one of the embedded themes in
`themes/`. Shape: `sifr_render::theme::Theme`
(`crates/sifr-render/src/theme/palette.rs`).

A theme names a `name` and a `[colors]` table of **seven required keys** plus
one optional eighth. Everything else the renderer reads — the selection band,
the `▶` marker, the `★` favourite marker, frames, dim text, the spectrum
gradient — is derived from those by `ColorsFile::derive`, so a theme author
picks hues, not roles.

One more top-level key is optional: `scanning_label` (default `"scanning…"`)
is the placeholder the playlist pane shows while the library is still being
listed. It covers the listing pass only — once the paths are in, the same
line switches to the tagging count the scan reports (`N tracks · tagging
64/N`), which is the code's own wording and not a theme's to set.

| key | required | role |
| --- | --- | --- |
| `bg` | yes | The desktop ground behind every panel. |
| `fg` | yes | The muted foreground: frames, rules, dim text. |
| `bright_fg` | yes | Body text, and the colour the selection band is mixed from. |
| `accent` | yes | The theme's own hue: chips, the `▶` marker, a queued row's `[q1]` chip, the spectrum's middle. |
| `green` / `yellow` / `red` | yes | The equalizer gradient, quiet to loud. `yellow` doubles as the secondary accent, worn by the `★` favourite marker. |
| `window_bg` | no | The panel ground. Omit it and `bg` is blended `6%` toward `fg`, which suits most palettes; name it when the palette already specifies the shade a panel sits at and a blend would drift off it (`gruvbox` uses Gruvbox's own `bg0_s`). |

Four derived roles are **contrast-corrected** rather than taken as written
(`crates/sifr-render/src/theme/contrast.rs`): the selection band is pushed away
from the panel ground until it is visibly a fill (≥ 1.5:1), `bar_groove` — the
unfilled half of the progress line and of the volume bar —
away from it on the same ladder and to the same ratio, the selected row's
text away from the band until it is readable on it (≥ 4.5:1), and the `▶`
marker away from both until its shape reads on either (≥ 3:1). This is why an
`accent` that happens to match the theme's own text — `oreo`'s does — still
gets a cursor you can see. `every_embedded_theme_has_a_visible_selection_band_and_marker`
holds every shipped theme to all three.

## `sifr-ui.toml`

Path: `$XDG_CONFIG_HOME/sifr/sifr-ui.toml`
(`dirs::config_dir().join("sifr").join("sifr-ui.toml")`, resolved by
`Config::appearance_config_path` in `crates/sifr-cli/src/config.rs` into
`ConfigPaths.appearance_path`) — the
same `sifr` directory `config.toml` lives in (see above), so both files
sit side by side. On macOS that's
`~/Library/Application Support/sifr/sifr-ui.toml`. Missing file or missing
directory both fall back to `AppearanceConfig::default()`; a parse error keeps the
last good config in memory and raises an error `Toast` for three seconds
(`report_source` in `crates/sifr-cli/src/tui/hot_reload/toasts.rs` sends
`WorkspaceRequest::SourceFailed { source: ConfigSource::Appearance, .. }`, which
`update` turns into `Workspace.toast`; the same error repeating raises nothing
further until its words change or the source recovers) rather than reverting
to defaults mid-session. If the platform has no config dir at all,
`appearance_config_path` falls back to the bare relative `sifr-ui.toml`, same
as before.

> **Migrating from an older sifr:** versions before this fix resolved
> `sifr-ui.toml` relative to the process's current working directory, so
> it was read and written wherever the binary happened to be launched
> from. That location is no longer read. If you have a `./sifr-ui.toml`
> sitting in a working directory you used to launch sifr from, move it to
> `$XDG_CONFIG_HOME/sifr/` (macOS: `~/Library/Application Support/sifr/`)
> — the same directory your `config.toml` already lives in, if you have
> one.
>
> A `theme = "..."` key here from an older sifr is now a parse error (it
> was unread since before this doc): theme selection is `theme` in
> `config.toml`, or the settings overlay's `Theme`/`Preset` rows. A `[keymap]`
> table here is a parse error too — `config.toml` is the one place actions
> are remapped.

### `[cover]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `plain \| vinyl \| milkdrop \| off` | `"vinyl"` | Cover art treatment. `mode` alone decides pixel vs. text: `vinyl`/`plain` render on the pixel path (dropping the cover column entirely when the terminal has no real graphics protocol); `milkdrop` never uses an image at all, and `off` draws no cover column at all — the card's text takes its whole width. |
| `text_cells.width` / `text_cells.height` | u16 / u16 | `20` / `8` | On-screen cell size reserved for `mode = "milkdrop"`; only consulted for that mode. The field scales to whatever cell it is given. |
| `brackets` | bool | `false` | Draw accent/dim corner brackets around whichever cover treatment is active (accent while `Player::Loading`, dim otherwise). |

### `[card]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `format_chips` | bool | `false` | Show bracketed format/bitrate/sample-rate chips (`[ MP3 ] [ 320 KBPS ] [ 44.1 KHZ ]`) on the time row. Not shown on `compact`/`minimal` layouts regardless. |
| `speed_chip` | `always \| changed \| never` | `"always"` | When the playback-speed chip shows: always, only when speed != 1.0x, or never. |

### `[progress]`

The thin progress line. `height_px`/`radius` shape the pixel renderer's
canvas; `fill`/`groove` are read by **both** renderers, so flipping `mode`
changes the shape of the bar and never its colour.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `height_px` | number, rounded to whole pixels | `4.0` | Line thickness (px) — deliberately thinner than the full cell height. |
| `radius` | optional number, rounded to whole pixels | unset (`height_px / 2.0`) | Pill-cap radius override. |
| `fill` | optional hex color | unset (falls back to theme `accent`) | Filled-portion color override. |
| `groove` | optional hex color | unset (falls back to the theme's derived `bar_groove` band — the window background pulled toward the body text until it clears 1.5:1, so the unfilled half is visible on every theme) | Unfilled-groove color override. |
| `mode` | `auto \| pixel \| text` | `"auto"` | Forces the pixel or text rendering path for the progress bar. |
| `remaining` | bool | `false` | Show a trailing `[ -03:01 ]` remaining-time chip at the row's right end. |

### `[layout]`

Terminal-size thresholds for the `Full`/`Compact`/`Minimal` layout choice.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `full_min_width` | u16 | `60` | Minimum terminal columns for the `Full` layout. |
| `full_min_height` | u16 | `19` | Minimum terminal rows for the `Full` layout. |
| `compact_min_width` | u16 | `30` | Minimum terminal columns for the `Compact` layout. |
| `compact_min_height` | u16 | `13` | Minimum terminal rows for the `Compact` layout. |
| `min_columns` | u16 | `48` | Terminal width floor below which drawing stops entirely and a "too small" message shows instead. |
| `min_rows` | u16 | `16` | Terminal height floor below which drawing stops entirely and a "too small" message shows instead. |
| `mode` | `auto \| full \| compact` | `"auto"` | Forces `select_layout`'s `Full`/`Compact` tier rather than letting it pick purely from terminal size — never below that tier's own minimum, so a forced tier too big for the terminal still falls back to whatever `auto` would have picked. `Ctrl+X` cycles this (`auto -> full -> compact -> auto`); the settings overlay's `Layout` row cycles it too. |

### `[window]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `window_colors` | bool | `true` | Repaint the whole terminal window (not just sifr's own cells) in the active theme's colors via OSC 10/11/12, on terminals that support it (Ghostty, kitty, iTerm2, WezTerm); ignored harmlessly elsewhere (e.g. Terminal.app). Also the settings overlay's `Window colors` row. |
| `key_hints` | bool | `true` | Draw the key-hint line (`components::organisms::key_hints::KeyHintsLine`) at all. `Ctrl+G` and the settings overlay's `Key hints` row both flip this; when `false`, `full`/`compact` skip that row and give the freed row back to the playlist pane. |
| `shadows` | bool | `false` | Every modal frame (KEYS, Settings, the search window, confirm-delete, jump-to-time, track info, the source-folder prompt, the `F12` trace) casts a one-cell drop shade one column right and one row down of its own box, in `░` (U+2591) painted in the theme's `dim` role — so a near-black theme like `noir` gets a *lighter* shade rather than an invisible one. The shade is clipped to the screen and never written over a cell a real graphics protocol owns (the cover image), for the same reason the modal itself moves clear of it. `Ctrl+G`-style chord: none — the settings overlay's `Shadows` row is the only toggle. |
| `key_buttons` | bool | `false` | The key hints draw as small bordered buttons (`╭───╮ / │ ⏎ │ / ╰───╯`, each with its own one-cell shade down its right edge) instead of the flat single-row chips. The key-hint strip grows from 1 row to 3 when this is on, which the playlist pane gives up; hints that no longer fit the row are dropped from the right rather than truncated. The settings overlay's `Key buttons` row toggles it. |
| `animations` | bool | `true` | The short transitions the terminal frontend stages over the drawn frame: an overlay opening and closing, the toast sliding in and dissolving, the card's rows reassembling on a track change, the transport chip pulsing on play/pause, the playlist status line on a shuffle/repeat change, a deleted row bursting, the cover cross-fading, and the whole-screen ones — the app starting, `Ctrl+X` cycling the layout, a theme change washing through. `false` stages nothing and drops whatever is already running on the next frame. The settings overlay's `Animations` row toggles it. Cover art and the pixel progress meter are never touched by an effect. |

<!-- defaults:window -->
```toml
[card]
format_chips = false
speed_chip = "always"

[progress]
height_px = 4.0
# radius = 2.0
# fill = "#rrggbb"
# track = "#rrggbb"
remaining = false

[cover]
mode = "vinyl"
brackets = false

[cover.text_cells]
width = 20
height = 8

[layout]
full_min_width = 60
full_min_height = 19
compact_min_width = 30
compact_min_height = 13
min_columns = 48
min_rows = 16
mode = "auto"

[window]
animations = true
key_hints = true
```
<!-- /defaults:window -->

## Regenerating the default blocks

Both `config_doc_test.rs` files extract the text between their file's
markers and parse it directly (`Config::parse` / `toml::from_str::
<AppearanceConfig>`), then assert the result equals that type's own `Default`.
If a default changes in code and the corresponding test fails:

1. Read the failing assertion's diff — it names the field(s) that no
   longer match.
2. Hand-edit the TOML between the matching `<!-- defaults:... -->` /
   `<!-- /defaults:... -->` markers in this file to the new value(s).
3. Re-run `cargo test -p sifr-cli -p sifr-render --test config_doc_test`
   until it's green again.

There's no `UPDATE_CONFIG_DOC=1`-style auto-rewrite: `Config` and
`AppearanceConfig` are `Deserialize`-only (see the note above), so there is no
`Serialize` impl this test could use to regenerate the block for you.

## `SIFR_TRACE` — the devtools trace

The one environment variable sifr reads, and the only way to turn the
devtools recorder's file output on. It is read once, at the composition root.

- `SIFR_TRACE=1` — timings only: one `<ms_since_start> <phase> <duration_ms>`
  line per timed phase, appended to `$HOME/.cache/sifr/trace.log`.
- `SIFR_TRACE=<path>` — the same timing log, **plus** the step stream: one
  JSON object per transition written to `<path>`, truncated when the process
  opens it, so one run is one session. A step names the machine, the message,
  the player/overlay/track/queue labels before and after, the effects emitted
  and whether anything moved at all:

      {"seq":42,"t_ms":8137,"us":38,"machine":"player","message":"playback_next",
       "before":{"player":"playing","overlay":"none","track":0,"queue":0},
       "after":{"player":"loading","overlay":"none","track":1,"queue":0},
       "effects":["audio:load#7","playback:play"],"outcome":"changed"}

  `message` is the same key the scenario runner's own table understands, so a
  recorded session converts into a `tests/fixtures/scenarios/*.toml` fixture
  and replays —
  `crates/sifr-core/tests/scenario/from_trace.rs` is that converter, and
  `from_trace_play_then_skip.toml` is a fixture nobody wrote by hand. **A bug
  report becomes a regression test by copying a file.**
- Anything else (unset, empty, `0`) — off. The recorder still keeps its last
  512 steps in memory either way (`sifr_runtime::devtools::TraceRing`); only
  the file output is gated.

The driver machines write to the same file. `LibraryWatch` (the library watch),
`ConfigWatch` (the config/theme watch) and `Decoding` (cover art) are `Update`
impls exactly as core's machines are, so a transition on the FSEvents thread
records the same shape of line — `machine` names the driver, `before`/`after`
are its own state names, and `effects` names the `*Io` its `perform` was
handed. Those steps carry `seq: 0` (there is no one counter four threads
share) and are ordered by `t_ms`; they reach the file only, never the
in-memory ring, which belongs to the event loop's thread. Each driver's
`perform` is also a timed phase in the same run
(`watch_perform`, `watched_perform`, `decode_perform`). The audio engine's own
machine lives in `sifr-audio`, a crate below the one that owns the sink, so it
is not recorded — its commands and events still show up as the `audio:*`
effects of the steps that emitted them.

**`F12` shows that in-memory ring** as a `TRACE` panel — the newest steps that
fit, one row each: milliseconds since start, the machine, the message, the
player state it moved between, how many effects it asked for, and `•`/`·` for
changed/unchanged. It needs no environment variable and no setting, it is not
in the keymap (`config.toml`'s `[keymap]` has no entry for it), and it works
with any overlay or text prompt already open — the key is intercepted in the
shell before key routing runs, because the panel is shell state rather than a
`Model.workspace.overlay` variant.

Nothing here is a feature flag: `sifr-core` has no `devtools` feature and
`update`'s signature is the same in every build. The shell wraps `dispatch`
(`sifr_runtime::devtools`), which is provably every message —
`crates/sifr-core/tests/guards/dispatch.rs` holds `update::update` to that one
call site.
