# Configuration

zefiro reads one TOML file, `config.toml`, from its own directory, plus theme
files.

| File | Parsed into | Purpose |
|---|---|---|
| `config.toml` | `TomlSettings` (`crates/config/src/config_file.rs`; the appearance tables in `crates/config/src/appearance_file.rs`) | music folder, theme, volume, audio, key rebinds; cover, card, progress line, layout breakpoints, window |
| `themes/<name>.toml` | `TomlTheme` (`crates/config/src/theme_file.rs`) | one colour theme |

## Rules for every file

- **The serde struct is the file's shape.** Every table sets
  `deny_unknown_fields`, so an unknown or misspelt key is a parse error.
  Each key is the field's own snake_case name.
- **Where the files live.** The directory is `dirs::config_dir()` joined with
  `zefiro`. On macOS that is `~/Library/Application Support/zefiro/`. If the
  platform has no config directory, startup fails with an error. There is no
  fallback to the current directory.
- **The first start writes a template.** When `config.toml` does not exist at
  startup, zefiro creates the folder and writes the template
  `crates/config/config.toml`, shipped in the binary: every key commented out
  at its default with a one-line note, under live, empty table headers, so a
  settings save fills the existing table and the user only uncomments keys.
  It then starts on `TomlSettings::default()`. An existing file, a symlink
  included, even a dangling one, is never touched. If the write
  fails, zefiro starts on the defaults and shows an error toast. A file
  removed during a session means the defaults, with no message.
- **A bad file falls back with a toast.** If `config.toml` cannot be read or
  does not parse at startup, zefiro starts with the defaults of both the
  settings and the appearance and shows one error toast. A bad theme file falls back to the stock theme the same way. During a
  session, a reload that fails keeps the last good values and shows a toast;
  the same error repeated raises no second toast.
- **Paths are taken as written.** zefiro does not expand `~` or `$VAR` in any
  value.
- **Files are watched, not polled.** The config driver watches the config
  directory (recursively, so `themes/` too) through a `notify` stream. On each
  change event it re-reads `config.toml` once and the active theme, and lists
  `themes/`. A file whose text did not change reports nothing.

## What reloads live

| Change | Effect |
|---|---|
| `config.toml` `[cover]`, `[card]`, `[progress]`, `[layout]`, `[window]`, any key | applies at once |
| the active theme file | applies at once |
| a new or removed file in `themes/` | the settings overlay's theme list updates |
| `config.toml` `[keymap]` | the key bindings are rebuilt |
| `config.toml` `music_dir` | if the folder changed, a full rescan starts |
| `config.toml` `theme`, `volume`, `[audio]` | read at startup only; an outside edit needs a restart |

The settings overlay (the `settings` action) changes values in the running app
and saves them into `config.toml`. A settings change (crossfade, ReplayGain,
output device, theme, volume, sleep presets, music folder) is a `ConfigPatch`;
an appearance change (cover mode, brackets, format chips, speed chip,
remaining time, key hints, animations, layout mode) is an `AppearancePatch`.
`crates/config/src/patch.rs` writes each patch with `toml_edit`: only the keys
the patch sets change, and comments, blank lines and table order stay.

The settings overlay's `Noir` preset sets the `noir` theme and these
`config.toml` values in one step: `[cover] mode = "milkdrop"`,
`[cover] brackets = true`, `[card] format_chips = true`,
`[progress] remaining = true` (`preset_appearance` in
`crates/kernel/src/domain/appearance.rs`).

## `config.toml`

Path: `<config dir>/zefiro/config.toml`.

The scanned library is cached apart from the config, under
`dirs::cache_dir()` joined with `zefiro` (macOS: `~/Library/Caches/zefiro/`):
`library.bin` holds the tagged tracks and `library.dir` the folder they were
scanned from. A missing or stale pair means a rescan.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `music_dir` | path | unset | Folder scanned for tracks. At startup the `--music-dir <PATH>` flag or the command-line `path` argument wins, then this key, then `dirs::audio_dir()`. If none resolves, or the folder does not exist, startup fails with an error. `--music-dir` checks that the path is a folder zefiro can read (otherwise zefiro exits with the reason and status 2, before the terminal is taken over), saves its absolute path as this key, keeping the file's comments, and starts on it; the `path` argument plays a folder for this run only and saves nothing. The source-folder overlay (the `music_dir` action) writes this key too. |
| `theme` | string | `"auto"` | Theme name. `"auto"` picks the stock theme, `noir`. Embedded themes: `terracotta-dark`, `terracotta-light`, `ember`, `gruvbox`, `gruvbox-light`, `hacker`, `macaroon`, `neobrutalism-dark`, `neobrutalism-light`, `noir`, `oreo`, `ristretto`, `rose-pine`, `rose-pine-dawn`, `wafer`, `winamp`. A file `themes/<name>.toml` in the config directory is offered too, and it shadows an embedded theme of the same name. An unknown name falls back to the stock theme with a toast. The `--theme` flag overrides this key. |
| `volume` | integer | `50` | Startup volume in percent, `0` to `100`. A value over `100` is a parse error. The `--volume` flag overrides it and also accepts only `0` to `100`. |
| `audio.crossfade` | duration string, `"Ns"` or `"Nms"` | `"0s"` | Crossfade between tracks, at most `10s`. `"0s"` is gapless. Only a track that ends on its own fades into the next; a skip cuts. |
| `audio.replay_gain` | bool | `false` | Apply ReplayGain volume normalisation. |
| `audio.device` | string | unset | Output device name as the settings overlay lists it. Unset means the system default output. |
| `audio.sleep_presets` | array of whole minutes | `[15, 30, 60]` | Sleep-timer lengths cycled by the `sleep_timer` action. At most 5 values, each `1` to `720`, strictly ascending; anything else is a parse error. The settings overlay cycles fixed bundles: `[15, 30, 60]`, `[10, 20, 45]`, `[30, 60, 90]`, `[45, 90, 120]`, or none. |
| `keymap.<action>` | chord string, or a table of `chord` and `context` | unset | Rebinds one action. An unset action keeps its built-in chord (`crates/kernel/src/update/keymap/table.rs`). An unknown action name is a parse error. |

The `[keymap]` actions are the snake_case names of `Action`
(`crates/kernel/src/domain/keymap.rs`): `play_pause`, `next`, `previous`,
`seek_back`, `seek_forward`, `seek_back_short`, `seek_forward_short`,
`seek_back_long`, `seek_forward_long`, `volume_up`, `volume_down`, `shuffle`,
`repeat`, `sleep_timer`, `ab_repeat`, `speed_down`, `speed_up`,
`jump_to_time`, `down`, `up`, `top`, `bottom`, `page_down`, `page_up`,
`play_selected`, `enqueue`, `play_next`, `dequeue`, `queue_move_up`,
`queue_move_down`, `cycle_sort`, `favorite`, `delete`, `save_playlist`,
`full_scan`, `track_details`, `search`, `history`, `settings`,
`settings_navigate_down`, `settings_navigate_up`, `settings_step_down`,
`settings_step_up`, `settings_activate`, `settings_close`, `music_dir`,
`help`, `quit`.

`music_dir` is left out of the block below: its default is unset, and the
folder comes from the OS at startup.

<!-- defaults:config -->
```toml
theme = "auto"
volume = 50

[audio]
crossfade = "0s"
replay_gain = false
# device = "MacBook Pro Speakers"
sleep_presets = [15, 30, 60]

[keymap]
# no overrides by default — every action uses its built-in chord
```
<!-- /defaults:config -->

### Key contexts

A `[keymap]` entry has two spellings. A chord string binds the action in its
own context, the one its built-in chords live in:

```toml
[keymap]
next = "N"
```

A table also names the context; a table without `context` keeps the action's
own context too:

```toml
[keymap]
next = { chord = "ctrl+n", context = "search" }
```

The contexts (`KeyContext`), and what is in focus in each:

| Context | In focus |
|---|---|
| `global` | the whole app |
| `playlist` | the browsing list |
| `text_prompt` | the save-playlist and add-server prompts |
| `music_dir` | the music folder prompt (Left and Right step through subfolders) |
| `search` | the search overlay |
| `help` | the help overlay |
| `history` | the history overlay |
| `settings` | the settings overlay |
| `confirm_trash` | the trash confirmation |
| `jump_to_time` | the jump-to-time prompt |
| `track_details` | the track info overlay |

While an overlay is open, only its own context applies; a `global` or
`playlist` binding does not reach through it (`docs/conventions.md` §3.8). A
binding that names an overlay's context overrides that overlay's built-in
chord.

An unknown context, an unknown key in the table, or a table without `chord`
is a parse error. A chord with bad syntax parses as a string and is reported
when the bindings are built.

### Appearance tables

The tables `[cover]`, `[card]`, `[progress]`, `[layout]` and `[window]` of
`config.toml` set the look. Any of their keys applies at once.

### `[cover]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `"vinyl"`, `"plain"`, `"milkdrop"` or `"off"` | `"vinyl"` | Cover treatment. `vinyl` and `plain` draw an image and size the cover from its aspect ratio. `milkdrop` draws text cells with no image. `off` draws no cover. |
| `cover_cells.width` / `cover_cells.height` | integer / integer | `20` / `8` | Cell size of the cover. Used only by `milkdrop`. |
| `brackets` | bool | `false` | Draw corner brackets around the cover and around the card's text column. |

### `[card]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `format_chips` | bool | `false` | Show format, bitrate and sample-rate chips on the time row. |
| `speed_chip` | `"always"`, `"changed"` or `"never"` | `"always"` | When the playback-speed chip shows. |

### `[progress]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `height_px` | number, rounded to whole pixels | `4` | Thickness of the pixel progress line. |
| `radius` | number, rounded to whole pixels | unset | Cap radius. Unset means half the height. |
| `fill` | `"#rrggbb"` | unset | Filled part colour. Unset means the theme's `accent`. |
| `groove` | `"#rrggbb"` | unset | Unfilled part colour. Unset means the theme's derived groove. |
| `remaining` | bool | `false` | Show the remaining-time chip at the row's right end. |

### `[layout]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `full_min_width` | integer | `60` | Fewest columns for the `Full` layout. |
| `full_min_height` | integer | `19` | Fewest rows for the `Full` layout. |
| `compact_min_width` | integer | `30` | Fewest columns for the `Compact` layout. |
| `compact_min_height` | integer | `13` | Fewest rows for the `Compact` layout. |
| `min_width` | integer | `48` | Below this many columns zefiro shows a "too small" message instead. |
| `min_height` | integer | `16` | Below this many rows zefiro shows a "too small" message instead. |
| `mode` | `"auto"` or `"compact"` | `"auto"` | Forces a layout tier. A forced tier that does not fit falls back to what `auto` picks. |

### `[window]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `animations` | bool | `true` | Play the short transitions (overlays, toasts, track change, theme changes). A theme change fades the whole screen at once in a quick 150 ms fade; a layout change shows at once with no transition. `false` stops them. |
| `key_hints` | bool | `true` | Draw the key-hint line. When `false`, the playlist gets that row. |

<!-- defaults:window -->
```toml
[card]
format_chips = false
speed_chip = "always"

[progress]
height_px = 4.0
# radius = 2.0
# fill = "#rrggbb"
# groove = "#rrggbb"
remaining = false

[cover]
mode = "vinyl"
brackets = false

[cover.cover_cells]
width = 20
height = 8

[layout]
full_min_width = 60
full_min_height = 19
compact_min_width = 30
compact_min_height = 13
min_width = 48
min_height = 16
mode = "auto"

[window]
animations = true
key_hints = true
```
<!-- /defaults:window -->

## Theme files

Path: `<config dir>/zefiro/themes/<name>.toml`, or one of the embedded themes in
`themes/` in the repository.

A theme has a `name`, a `[colors]` table of seven required keys and one
optional key, and an optional `scanning_label`. The renderer derives every
other role (selection band, markers, frames, dim text, spectrum gradient) from
these colours.

| Key | Required | Role |
|---|---|---|
| `background` | yes | The ground behind every panel. |
| `muted_foreground` | yes | Frames, rules, dim text. |
| `foreground` | yes | Body text; the selection band is mixed from it. |
| `accent` | yes | The theme's hue: chips, the `▶` marker, the queue chip, the spectrum's middle. |
| `green` / `yellow` / `red` | yes | The spectrum gradient, quiet to loud. `yellow` is also the `★` favourite marker. |
| `window_background` | no | The panel ground. If unset, `background` is blended 6 % toward `muted_foreground`. |
| `scanning_label` | no (top level) | Text the playlist pane shows while the library is listed. Default `"scanning…"`. |

Each colour is a `"#rrggbb"` string.

Some derived roles are contrast-corrected
(`crates/widgets/src/theme/contrast.rs`): the selection band against the panel
ground (at least 1.5:1), the bar groove against the panel ground (1.5:1), the
selected row's text against the band (4.5:1), and the `▶` marker against both
(3:1).

## Regenerating the default blocks

Two guards in `crates/zefiro-guards` lock the blocks above to the code.
`config_doc_config.rs` parses the `defaults:config` block with `parse_config` and
compares it with `TomlSettings::default()`. `config_doc_appearance.rs` parses the
`defaults:window` block with `parse_config` too and compares it with
`TomlSettings::default()`. The structs are `Deserialize` only, so the blocks
are kept by hand. When a default changes:

1. Read the failing assertion's diff; it names the fields that differ.
2. Edit the TOML between the matching markers in this file.
3. Run `cargo test -p zefiro-guards` until it passes.
