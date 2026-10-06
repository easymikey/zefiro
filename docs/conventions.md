# Conventions — the rulebook

Every rule of this codebase lives here, once. Other docs link here instead of restating a rule: `docs/principles.md` says why, `docs/architecture.md` shows the structure, `docs/testing.md` shows how tests are laid out. Where another doc disagrees with this file, this file wins.

Every rule is marked `guard` (a clippy lint or a `sifr-guards` test holds it; a guard rule with no guard yet is a guard task) or `review` (needs judgement; a reviewer checks it, §14). In tables the mark is the `check` column; in lists it ends the rule.

Decisions log: decided 2026-10-02/03 with the user; the work that brings the code to these rules is the rename batches in `.work/reviews/glossary-draft.md` §7, queued in `.work/briefs/waves.md`. Questions still open are §15: do not settle them while editing.

## 1. Flow

```
shell XRequest ─► Message ─► update(&mut Model, Message, Moment) ─► Cmd { effects: [Effect], messages: [Message] }
Effect::X(XCmd) ─► runtime ─► DriverLoop ─► XMessage::Cmds(Cmds { cmds, at }) ─► XDriver::transition ─► Cmd<XEffect, XEvent>
XEffect ─► XDriver::execute          XEvent ─► DriverLoop ─► inbox ─► update
```

1. Layer map: kernel: nothing; audio, library, macos, config: kernel; runtime: drivers, kernel, config; widgets: kernel; terminal: kernel, widgets; sifr: anything. `guard` (`layering.rs`)
2. kernel and widgets are pure: no IO, clock, threads, env, channels. `guard` (`purity.rs`)
3. Only the roots see a whole `Model`: the kernel router inside `update`, `startup`, `Scene::from_model`. Below them a function takes its slice or an `XParts`. `guard` (`demeter.rs`, `demeter_views.rs`)
4. One entry each: `update` has one call site in runtime, there is one key router, one `startup`; the paint path never calls `update`. `guard` (`dispatch.rs`)
5. Effects are data with one interpreter: the kernel returns `Effect`s, runtime runs each as a lookup with no decision, the shell does only terminal IO; no second place matches on `Effect`. `review`
6. Calc and effect are split: every decision is a pure function with tests; the action beside it holds no logic. `review`
7. No loop through IO for our own changes: a value a message changes is changed in the `Model` in that `update`; persisting it is an effect; a watcher exists only for external edits and never re-applies what the app just wrote. `review`
8. Derived state lives next to its source: a cache is keyed by an explicit generation or `Revision` and owned by the crate that computes it (widgets and terminal own pixel resources), never synced field by field per frame by the shell. `review`

## 2. Entity shapes

| kind | crate | required shape | name | check |
|---|---|---|---|---|
| Model | kernel | the only app state; owned slices (`player`, `transport`, `playlist`, `workspace`, `queue`, …); `startup(Startup) -> (Model, Vec<Effect>)` (drains its messages like `update`); `update(&mut Model, Message, Moment) -> Result<Vec<Effect>, Unhandled>` (drains `Cmd` messages itself, §3.4) | `Model`; first call `startup` | review |
| Machine | kernel (trait), implementors anywhere | `trait Machine { type Message; type Effect; fn transition(&mut self, message: Self::Message) -> Result<Self::Effect, Unhandled>; }`; `Effect` is a `Cmd` type (kernel `Cmd`, driver part `Cmd<XEffect, XEvent>`); the contract (`Machine`, `Driver`, `Unhandled`) lives in `kernel::update::machine`, effects in `kernel::cmd` | see §3 | review |
| Parts | kernel | `struct XParts<'a>` whose fields are only `&`/`&mut` borrows of `Model` fields, built by the router | `XParts` | review |
| Cmd | kernel | `struct Cmd<E = Effect, M = Message> { effects: Vec<E>, messages: Vec<M> }`; build with `Cmd::none()`, `Cmd::effect(e)`, `Cmd::message(m)`, `From<Effect>`, `From<Cue>`, `FromIterator<E>`; join with `then` (appends, in order); read with `effects()` / `IntoIterator`, split with `into_parts()` (runtime reads a driver Cmd); nested machines lift with `map_effect` / `map_message`. No other combinator (`merge`, `chain`, `batch`, `and`) | `Cmd` = what a machine returned | guard |
| Effect | kernel | one variant per target: `Audio(AudioCmd)`, `Library(LibraryCmd)`, `Macos(MacosCmd)`, `Config(ConfigCmd)`, `WindowColors(WindowColorsCmd)`, `Animate(Cue)`, `RollShuffle(usize)`, `After { delay, timer }`, `Restart(DriverName)`, `Quit` | `Effect` | review |
| XCmd | kernel | the order to one driver, carried inside `Effect` | `AudioCmd`, `LibraryCmd`, `MacosCmd`, `ConfigCmd` | guard |
| Event | kernel (types) | what a driver reports; `From<XEvent> for Message`; driver lifecycle is `DriverEvent { Died, Stopped, Full }` | `XEvent` | guard |
| Answer | kernel | runtime's reply to a kernel effect it ran: a `Message` variant named for the effect in the past tense (`Effect::RollShuffle` → `Message::ShuffleRolled`) | past tense of the effect | review |
| Request | kernel | what the shell asks the core; always a branch of `Message` | `XRequest` | guard |
| Message | kernel | the only input of `update`: `X(XRequest)`, `X(XEvent)`, answers, `Elapsed(Timer)`, `Driver { driver, event }`, `Key(KeyPress)`, … | `Message` | review |
| Driver | audio, macos, library, config | the top machine of one external source (§4) | `AudioDriver`, `MacosDriver`, `LibraryDriver`, `ConfigDriver` | guard |
| DriverLoop | runtime | one generic loop, one thread per driver (§4); a driver effect reaches it through `LoopEffect { Execute, Run, After, Watch, Unwatch }` (decided 2026-10-04); runtime seeds `XMessage::Started` into the inbox at spawn through the `DriverLoop` field `seed: Option<D::Message>`; the loop's private next-input enum is `LoopInput` (not `Wake`, reserved for the realtime wake-up); the inputs `Spawners` hands each driver thread are `SpawnSetup`, the audio start closure `SpawnAudio`; an audio driver that stops before handing over its tap is `Error::TapLost { driver }`; a value the driver publishes goes out through a closure sink `P: Fn(T)` the runtime passes in | `DriverLoop` | guard |
| Stream | runtime | a repeated input started by a driver effect (`Watch(PathBuf)`, as Crux `stream_from_shell`); runtime owns it and feeds its items back as `XMessage`s. The word `Subscription` is not used | `FileStream` | guard |
| Job | driver crate (type), runtime (thread) | slow blocking work a driver hands to the runtime worker as `XEffect::Run(XJob)`; the result returns as an `XMessage`; stale by `Revision` (§4.7) | `AudioJob`, `LibraryJob`, `MacosJob` | review |
| Error | every crate that can fail | §6 | `Error`, `<Type>Error` | guard |
| Scene | widgets | `Scene::from_model(&Model, ScenePresentation)`; the only widget code that sees `&Model`; no `*_view()` or `layout_parts()` getters | `Scene` | guard |
| View | widgets | read-model holding fields from two or more `Model` slices; borrowed fields, no state, no `&Model`; built only by `XView::from_scene(&Scene)` | `XView<'a>` | review |
| Widget | widgets | every type with `impl Widget`, overlays included; every widget type ends in `Widget` (`ToastWidget`, `CardWidget`, `TooSmallWidget`); built as in ratatui and ratcn: `XWidget::new(..)` takes what the widget cannot paint without (its `input`, then the `ActiveTheme` when it paints in theme colours), every optional knob is a consuming setter named after the field (`style(XStyle)`, `speed_chip(SpeedChip)`, …); fields are private, so there is never a struct literal outside its module and never a `builder()`; `input` is `&` one Model slice or one `XView` | `XWidget`, never `*Overlay` | guard |
| Style | widgets | a component's look; built only by `XStyle::from_theme(&ActiveTheme)`; an input beyond the theme rides on `ActiveTheme` through a builder (`with_progress`, `with_volume_pulse`); fields are semantic colours (`foreground`, `muted_foreground`, `background`, `border`, `accent`, …) | `XStyle` | review |
| Colors | widgets | only the theme palette | `Colors` | guard |
| raw TOML | config | every serde shape of a file or a section; each carries `#[serde(expecting = "…")]` in user words (`"a [cover] table"`), so a Rust name never reaches a toast | `Toml*` (`TomlTheme`, `TomlAppearance`, `TomlKeymap`, `TomlCard`, `TomlColors`, `TomlAudio`) | guard |
| parsed value | kernel, widgets | what inner code uses; parsed once at the boundary, never re-checked | bare noun (`Theme`, `Keymap`, `Appearance`) | review |
| user settings | kernel | values the user edits in a file or the settings overlay | `XSettings` (`Settings`, `AudioSettings`, `AppearanceSettings`) | review |
| config file id | kernel | `ConfigName { Config, Appearance, Theme(ThemeName) }` | `ConfigName` | review |
| read result | config | the outcome of reading and parsing one file at startup | `Parsed<T>` | review |
| test-table row | tests | one row of an `rstest` table | `XRow` | review |

## 3. Machines

1. Every part that reacts to messages is `impl Machine` with `transition` (Crux: every part is `update`). There is no second form; the word `apply` is not used. `review`
2. `transition` returns `Result<Cmd, Unhandled>` (the `Cmd` type as above), never `Option`, `Vec`, a bare effect enum or an `*Outcome`. `pub struct Unhandled;` (kernel, one for the whole workspace, no reason inside, as statig/XState "unhandled" and rust-fsm `TransitionImpossibleError`): a message with no transition in the current state returns `Err(Unhandled)` and leaves `self` untouched; `Ok(Cmd::none())` means handled with nothing to do outside. Runtime reads it: `Ok` repaints, `Err` does not repaint and restores the toast the key dismissed; a driver loop drops `Err(Unhandled)` silently (nothing to repaint); so do the kernel drain for a refused follow-up `Cmd` message and the runtime for a refused startup or answer delivery, the envelope frame loop and the painter (the whole sanctioned list). No per-machine error enums. `review`
3. The input is an enum `XMessage` where X is the machine's type name (for an impl on `Option<T>` or `CursorOver<T>`, the noun of `T`; for a top driver machine, the subsystem: `AudioMessage`, `MacosMessage`, `LibraryMessage`, `ConfigMessage`), even with one variant, so a new input source is one new variant. Exception (Crux: no parallel enum that maps 1:1): when the machine's input would be identical to a shell `XRequest` enum, the machine takes that `XRequest` as its `Machine::Message` (`CursorOver<SearchQuery>` takes `SearchRequest`, `TextEntry<E>` takes `TextRequest`). The parameter is always `message`. `guard`
4. A message to self or parent is `Cmd::message(m)`; no follow-up or out-message types. DECIDED 2026-10-03 (as Crux `process_event`): kernel `update` drains every `Cmd` message itself, depth-first, in order, inside the same call, and returns only the collected effects, so a step and its follow-ups are atomic, no frame sees a half-applied model, and kernel tests see the final state through the production `update`. Depth over 8 is a programmer error: `debug_assert!`, and release stops the chain. `review`
5. The match is exhaustive, no `_ =>`. A variant move uses one `mem::replace`; `mem::take` on machine state is banned. `guard` (`_ =>`), `review` (`mem::take`)
6. A machine reads no clock. Kernel time arrives as `Moment`; driver commands arrive as `XMessage::Cmds(Cmds { cmds, at: Instant })` (kernel `pub struct Cmds<C> { cmds: Vec<C>, at: Instant }`; `DriverLoop` needs `D::Message: From<Cmds<C>>`) (`DriverLoop` reads the clock; time as data, as `crux_time`). `review`
7. A part is a plain noun naming what it works on, with no `State` suffix: `Engine`, `Hardware`, `Cover`, `ConfigWatch`, `CoverDecoding`. No two public types in the workspace share a name, except each crate's boundary `Error`. `guard`
8. Keys go to one key context: every binding names its `KeyContext`; an open overlay takes every key (a typed letter never reaches a global command), and with no overlay the router tries playlist, then global. `Esc` and `q` close every overlay through ordinary `Close` bindings, not router special cases (decided 2026-10-05). `review`
9. A machine on a realtime thread (cpal callback: `Envelope`) has `Effect = ()`: `transition(&mut self, message) -> Result<(), Unhandled>`, so it allocates nothing. `guard`

## 4. Drivers

Why `Driver`: same roles as OS drivers (request in, interrupt-driven events out, no driver-to-driver calls; DriverKit: the system owns queues and threads). Not `Actor`: drivers never message each other.

1. Each driver has one top machine `XDriver` that implements `Driver` (§4.5a). Its input `XMessage` has the variant `Cmds(Cmds<XCmd>)` plus one variant per own signal source (job results, stream items, callback input). `review`
2. `XDriver::transition` returns `Cmd<XEffect, XEvent>`: `effects` are actions, `messages` are reports to the kernel. An `XEffect` enum has no variant carrying an event. DECIDED 2026-10-03 (Crux `map_event`): `messages` always go one level up to the parent, at every level; a part inside a driver returns `Cmd<PartEffect, XMessage>` (`ConfigWatch` → `Cmd<WatchEffect, ConfigChange>`, which `ConfigDriver` lifts) and the driver lifts it with `map_effect`/`map_message`; the top driver's parent is the kernel, so its messages are `XEvent`. A machine never messages itself; an IO answer comes back through `execute` (§4.3). No driver builds a kernel `Message`. `review`
3. The only impure step is `XDriver::execute(&mut self, effect: XEffect) -> Option<XMessage>`. `None` = fire-and-forget (Crux `Output = ()`); `Some(answer)` = the IO result, which `DriverLoop` feeds to `transition` at once, before the next inbox item (Crux `resolve`). The answer to effect `X` is the message variant named for it in the past tense (`Open` → `Opened`, `Save` → `Saved`, `Read` → `ReadDone`). Words `perform`, `handle`, `process`, `dispatch`, `apply` are banned for it. `guard`
4. Data a driver needs at start are fields of `XDriver`, not a `*Parts` bundle. `review`
5a. DECIDED 2026-10-03 (Crux: effects start everything, the shell owns the loop): kernel `pub trait Driver: Machine { type Effect; fn execute(&mut self, effect: Self::Effect) -> Option<Self::Message>; }` is the only driver trait; kernel `enum Driver` (which driver) becomes `DriverName` (as `OverlayName`, `ConfigName`). No per-driver hooks in `DriverLoop`: a deadline or debounce is an effect `After { delay, timer }` (as kernel `Effect::After`, Crux `notify_after`); watching files or any repeated input is an effect that starts a stream (`Watch(PathBuf)`), whose items come back as `XMessage`s; shutdown is an ordinary kernel command sent before `Quit` (`ConfigCmd::Flush`), not a loop hook; `DriverLoop` drains the inbox and delivers all pending commands as one message `XMessage::Cmds(Cmds<XCmd>)`, so coalescing (keep the last volume) is pure machine logic tested by tables; `DriverLoop::spawn(start: impl FnOnce() -> D + Send)` builds the driver on its own thread (rodio output is `!Send`). Rejected: Elm-style `subscriptions()` (a second mechanism beside effects). `review`
5. A driver opens no thread and no channel. `DriverLoop` (runtime) opens and closes every thread, channel, worker and stream: receive, read the clock, `transition`, `execute` each effect, send each event as a `Message` into `inbox`. There is no per-driver loop type. Threads a library opens inside itself (rodio output, `notify` watcher) are excepted. `review`
5b. DECIDED 2026-10-03: a thread the OS or a library owns (AppKit main thread, CoreAudio listeners, cpal/rodio output) is a callback, never created or joined by us. Runtime opens a channel into the driver's inbox and hands its sender to whoever registers the callback; the callback only parses its input (§4.6) and sends an `XMessage`; every decision is in the driver machine on its own thread. The AppKit main loop `MainLoop` is started by runtime `host` (the name follows AppKit; allowed `Loop` types are in §9). `review`
6. Input from a callback that must answer synchronously (AppKit remote commands) is parsed at the boundary and decided in the machine: `RemoteInput::parse(trigger, event) -> Result<RemoteInput, RemoteInputError>`; a parse error answers `CommandFailed` at once, a parsed input is sent as `MacosMessage::Remote(input)` and answers `Success`. `review`
7. An `Effect` is what a machine asks to be done outside; it runs at once (`execute` for a driver, runtime for the kernel). A `Job` is slow work for a worker thread, carried as `Run(XJob)` (not `Queue`: queue is the play queue); its result returns later as an `XMessage`. A job in flight carries a `Revision`; a result whose `Revision` is stale is dropped. `review`
8. Nothing shared crosses a driver boundary: values cross as owned messages or through latest-value cells; no `Mutex`/`RwLock` handed out. A plain fn the composition root hands a driver (`library::cover::cover_bytes` for macos covers) is code, not shared state. `review`
9. Platform code lives in its own crate (`macos`), a `[target.'cfg(target_os = "…")'.dependencies]` entry, never a plain dependency; with one backend per target the choice is compile-time, no trait object. `review`
10. Adding a driver is this checklist, nothing else: a `DriverName` variant; its `runtime::registry` `DriverRow` (thread name, hosting, platform) and its `Supervision::standard` arm; its port in `Ports`; `XCmd` and the variant `Effect::X(XCmd)` with its interpreter arm; `XEvent` and `From<XEvent> for Message`; `XMessage` with `Cmds(Cmds<XCmd>)`; `impl Machine` and `impl Driver for XDriver`; `DriverLoop::spawn(start)` in wiring; a table test of `transition`. Wiring outside these places is a defect. `review`

## 5. Effects and enum variants

1. Variant shape by field count: no data → unit (`Play`); one field → tuple (`Seek(Duration)`); two or more → named fields (`After { delay, timer }`). A single named field is banned, with two exceptions: the context field of an error variant (`Device { requested: String }`, as thiserror) and a row count with no type of its own (`CursorBy { rows: i32 }`). A unit goes into the type, not the field name: time is `Duration` (`OutOfRange(Duration)`; a signed step is `{ direction: Direction, by: Duration }`); a count is a tuple, the variant name says what it counts (`RollShuffle(usize)`). `guard`
2. An effect carries only the data its action needs. A compound action is several effects in one `Cmd`, in order (`Open(device)` then `SetSpeed(speed)`), as Crux `Command::all`/`then`. `review`
3. An effect is a whole desired state or carries a `Revision`; effects are absolute (`SetVolume(…)`), so a repeat is harmless. Stale timers are dropped by `Revision`; `Effect::After { delay, timer }` stays (as `crux_time` `NotifyAfter`). `review`
4. A newtype exists when the value has a rule (range, invariant: `Speed(f32)`), a unit (`Cells`, `Pixels`) or an index space (`TrackIndex`, `ViewIndex`); a bare integer in geometry is a defect; every `as` cast sits in a named conversion: a unit conversion is a method on the newtype that owns the unit (`Frames::duration(rate)`), a plain numeric cast is a fn named by its result (`bin_index`), never `x_to_y`. `review`
5. Use std types where they say it: `Result<..>` and `Option<..>` written out; no `Result` aliases (`type XResult = …`). `guard`

## 6. Errors

1. Every crate that can fail has one boundary `Error`. Machines and `update` return `Unhandled` (§3.2), not an error type. `guard`
2. An error of parsing or validating one value is `<Type>Error` (`ThemeNameError`, `TimecodeError`, `RemoteInputError`); its out-of-range case is the variant `OutOfRange { value, max }` (plus `min` when the floor is not zero). A panic payload is not carried: `DriverError::Panicked` is a unit variant; the payload text is dropped until runtime devtools exist (§6.4). `guard`
3. `thiserror` enum, `#[error]` user text on every variant, `#[source]` chains, context fields, no `String` payloads (config reload: §8; `ConfigError: PartialEq` so a toast shows only for a new error; user text comes from `Display` when the toast is built), no `Box<dyn Error>`, no `anyhow`, `From` only along a real crate edge. No `let _ =` anywhere, tests included (a value is handled, propagated, asserted or turned into an event), no `unwrap_or_default` hiding an error. `guard` (`errors.rs`)
4. IO failures travel as data inside an `XEvent` into the Model and a toast, never as a panic. A message or event enum carries an error in one `Error(XError)` variant (`AudioMessage::Error(AudioError)`, `LibraryEvent::Error`); no per-operation `*Failed` variants — the machine branches on the error variant (decided 2026-10-04). Debugging is later devtools in runtime recording `Message` → `Cmd` and the model before/after (Elm debugger style). `review`

## 7. Suffixes and prefixes

The meaning of each affix is `review`; the bans are in §9.

| affix | meaning | example |
|---|---|---|
| `Cmd` | `Cmd` = machine result; `XCmd` = order to one driver | `Cmd`, `AudioCmd`, `WindowColorsCmd` |
| `Effect` | an action asked of the outside | `Effect`, `EngineEffect`, `LibraryEffect` |
| `Event` | a fact a driver reports | `AudioEvent`, `DriverEvent` |
| `Request` | shell → core only, a `Message` branch | `PlaybackRequest`, `OverlayRequest` |
| `Message` | a machine's input | `PlayerMessage`, `LibraryMessage` |
| `Driver` | top machine of one driver | `AudioDriver` |
| stream (concept, no suffix) | repeated input started by a driver effect | `FileStream` |
| `Job` | slow work for a worker thread | `AudioJob`, `LibraryJob` |
| `Watch` | a driver part that watches files | `ConfigWatch` |
| `Error` | error enum | `Error`, `TimecodeError` |
| `Parts` | `Model` borrows only | `PlaybackParts`, `ResyncParts` |
| `View` | Scene read-model of 2+ slices | `CardView` |
| `Widget` | type with `impl Widget` | `ToastWidget`, `CompactCardWidget` |
| `Style` | component look from `from_theme` | `VinylStyle`, `MilkdropStyle` |
| `Settings` | values the user edits | `AudioSettings`, `AppearanceSettings` |
| `Toml` (prefix) | raw serde shape | `TomlTheme`, `TomlCard` |
| `Name` | which one of a closed set | `OverlayName`, `ConfigName`, `ThemeName`, `DriverName` |
| `Row` | settings / registry / test-table row | `SettingRow`, `DriverRow` |
| `Patch` | partial change to a stored value; every field an `Option`, built by struct update `XPatch { field: Some(v), ..XPatch::default() }`, no builder crate | `ConfigPatch`, `AppearancePatch` |
| `Index` | position newtype in one index space | `TrackIndex`, `ViewIndex` |

## 8. Domain words

Editor-shaped concepts take Zed's word (`Theme`, `Keymap`, `Workspace`, `Toast`; Zed's modal is our `Overlay`). The "not" column is held by `RETIRED_NAMES` (`guard`); picking the word for a concept is `review`.

| concept | the word | not |
|---|---|---|
| track identity everywhere (queue, favorites, history, m3u) | `TrackRef` (`enum TrackRef { Local(PathBuf) }`, `Remote { source, id }` with Navidrome; as MPD songid, Subsonic id); `Local` keeps today's path behaviour: no normalisation, same path text on disk, lookup through a `TrackRef` to index map, a dangling ref is skipped on load, field `Track.source: TrackRef` | a path or an index as identity |
| track position in the library / the shown list (rows and cursor only) | `TrackIndex`, `ViewIndex` | `PlaylistIndex`, `QueueIndex` |
| audio load order | `TrackLoad` | — |
| audio's preload variant (gapless or crossfade) | `PreloadMode` (decided 2026-10-04) | `PreloadKind`, `Preload` |
| who paused playback | `PausedBy` | — |
| modal surface in the core | `Overlay`, `OverlayName`; render frame and geometry `Modal*` | `OverlayKind`, `OverlayScreen`, `Pane*` (only the `playlist::pane` module) |
| short-lived message | `Toast` | `Notice` |
| bottom key line | `KeyHints` (`key_hints`) | `Footer` |
| small label | chip (`format_chips`, `speed_chip`) | badge, `tech_chips` |
| preload-due / A-B-end timer | `Lookahead` (`Timer::Lookahead`) | `Mark` |
| A-B point | `Mark` (`AbLoop::mark`) | — |
| render part of `sifr-ui.toml` | kernel `domain::appearance::Appearance` (`CoverCells`, `Breakpoints`, `ProgressBar`; built by `TomlAppearance::appearance`) | `Look`, `Custom*`, `UiOptions`, `[ui]` |
| choices the settings overlay edits | kernel `AppearanceSettings` (field `Settings.appearance`) | — |
| how the cover is shown | `CoverMode` (key `cover_mode`) | `CoverStyle` |
| step a setting / volume / speed | `Step` + `Direction { Next, Previous }` (`Message::Step { row, direction }`, `StepVolume(Direction)`); size is a constant beside the value (`VOLUME_STEP = 5`) | `Adjust`, `Nudge`, `steps: i8` |
| absolute input (remote, IPC, macOS) | `Set*(value)` (`SetVolume`, as cliamp) | — |
| colour scheme / palette | `Theme`, `Colors` | `Palette`, `Skin` |
| terminal window bg/fg | `WindowColors` | `WindowTint` |
| keys | `Keymap`, `KeyBinding`, `Chord`, `KeyContext`; a binding maps to a `Message`; an unbound `Char` in a text `KeyContext` is typed input in lookup (Zed) | `Keys`, `Focus`, `KeyEffect`, `KeyOutcome`, `CharSink`, `AnyChar` |
| replay gain | `ReplayGain` | `Replaygain` |
| seek by tenths | `SeekTenths` | `SeekFraction` |
| timer staleness | `Revision`, `Freshness` | `Reply` |
| driver mailbox overflow | `DriverEvent::Full` | `Congested` |
| realtime wake-up of a driver | `Wake { Sent, Pending }`, fn `wake`; job staleness is a `Revision`, as timers | `Unsent`, `ring`, `Ticket` |
| frame pipeline | `prepaint` → `FrameLayout`, `Scene`, `XWidget::render` (ratatui trait methods only), `paint*` (own fns writing into `Buffer`/`Canvas`/`Pixmap`) | `draw`, `render` for own fns or for building |
| values `Scene` takes beside the `Model` (theme, colour depth, bindings, …) | `ScenePresentation` | — |
| time passed into the kernel | `Moment` | — |
| first model | `startup`, `Startup` | init, `boot`, `Boot` |
| everything the binary reads before the runtime starts (first model, paths, theme) | `Launch { startup, paths, theme }`, `launch()` (decided 2026-10-04) | `Boot`, `Look` |
| terminal input for one cover refresh (layout, crossfade, wash) | `CoverRefresh` (decided 2026-10-04) | `CoverRefreshParts` |
| a parser's message shown to the user (TOML error text crossing into the pure kernel) | `Diagnostic(String)`, built only by `Diagnostic::from_error(&impl Error)` in the crate that owns the parser; kernel `ConfigError::Invalid(Diagnostic)` (decided 2026-10-04) | `detail: String` |
| linear amplitude factor in audio | `Gain(f32)` (decided 2026-10-04); a replay-gain tag value is `Decibels(f32)` in kernel, converted to `Gain` in audio | bare `f32` volume or gain |
| terminal geometry unit | kernel `domain::geometry::{Cells(u16), Pixels(u32)}` (columns and rows alike; pixel sizes), imported by widgets, library and sifr (decided 2026-10-04, moved from widgets/library so `Appearance` and `visible_rows` can use them); ratatui `Rect`/`u16` stay at the ratatui boundary only | bare `u16`/`usize`/`u32` sizes |
| index into the sleep presets | `PresetIndex` (`Index` row) | `preset_index: usize` |
| row position in a settings or history overlay | `RowIndex` (`Index` row; decided 2026-10-04) | `selected: usize`, `ViewIndex` (shown track list only) |
| macOS `OSStatus` code | `OsStatus(i32)` | bare `i32` |
| progress bar's unfilled part | `groove` (as `Role::BarGroove`) | `track` (collides with `Track`) |
| bad colour text in appearance | `ColorError::Malformed(Diagnostic)` | `input: String` |
| overlay content messages inside `OverlayMessage` | `OverlayContentMessage` | `InnerMessage` |
| default key bindings | Rust tables in kernel `update/keymap` (data in code by decision 2026-10-04) | embedded TOML |
| a child module's message handler | `update` (Elm), beside the root `update` | `step`, `handle` |
| audio engine with no device open | `Engine::Closed` | `Muted` (reads as volume mute) |
| deck's realtime wake-up signal | `DeckEvent::Woke(Revision)`, fn `wake` | `Track(Revision)`, `notify` |
| macOS main-loop parts | `MainLoopStop`, `NowPlaying`, `NowPlayingClock`; `HardwarePoll`, `CoverReader` keep their names | `LoopStopper`, `Panel`, `PanelClock` |
| terminal app identity, pixel protocol | `TerminalApp`; `PixelProtocol { Kitty, Iterm2, Sixel, Query }`; `Capabilities::from_environment`; `CoverPainter` | `Brand`, `Protocol { Kgp, Iip, Probe }`, `before_probe`, `CoverRenderer` |
| the kernel mailbox sender, everywhere | `inbox` | `sender`, `report_sender`, `receiver` |
| config watch sub-machine step inside `ConfigDriver` | `drive_watch`; never-read file state `Seen::Unread` | `drive`, `Seen::Never` |
| job coalescing in `DriverLoop` (keep the newest job per kind, run in `Ord` order) | allowed `DriverLoop` duty (scheduling, not a decision about the result; staleness stays `Revision` in the machine) (decided 2026-10-04) | — |
| discarding an `io::Result` | allowed only inside `Drop` and the panic hook, as `drop(result)` (nowhere to report) | anywhere else; `.ok();` |
| card cover state in widgets; decoded cover pixels in widgets | `CardCover`; `CoverImage` | `CoverArt` (library's), `DecodedCover` |
| library disk orders inside `LibraryEffect::Execute` | `DiskEffect` (as `WatchEffect`) | `LibraryCmd` reused |
| input the shell feeds runtime (keys, resize, paint failures) | `ShellInput` | `ShellEvent` (Event = driver fact) |
| named `Rect`s of one `FrameLayout` part | suffix `Areas` (`ModalAreas`, `PlaylistAreas`, `ToastAreas`) | `Rects`, `Regions` |
| read-models `KeyHintsContent`, `OverlayContent` | `KeyHintsView`, `OverlayView` (View row) | `Content` suffix |
| theme input colours before derivation | `ThemeBase` | `ThemeSeed` (`Seed` = DriverLoop seed) |
| sifr's values beside the Model for `ScenePresentation` | `ShellPresentation` | `Presentation` |
| the cover-crossfade state machine (widgets `pixels::cover::gate`) | `CrossfadeGate`, `CrossfadeGateMessage`; `CoverArrival`; `Motion`, library `JobPriority` keep their names | `PendingCrossfade`, `Advance` |
| CPU-parallel work inside one job (tag reading) | allowed: `thread::scope` inside a job body, joined before the job returns (decided 2026-10-04) | detached threads in jobs |
| turning the raw `TomlTheme` into the widgets `Theme` | the shell (sifr) does it: `Theme` is a widgets type and config sits below widgets; config publishes `TomlTheme` (decided 2026-10-04) | config depending on widgets |
| which cover to decode and at what size | the kernel decides: the shell reports the laid-out cover side through `Message::Viewport` (`Pixels`), kernel emits `Effect::Library(LibraryCmd::DecodeCover(CoverJob))` on track or side change; no shell→driver side channel (decided 2026-10-04) | paint path sending `LibraryMessage::Cover` |
| config reloaded | kernel `ConfigReload { name: ConfigName, result: Result<(), ConfigError> }` (§6.3), `config_reloaded`; the per-file errors held by the workspace are `ConfigErrors`, field `config_errors`; startup shows one toast with the first error and records the rest; the live values parsed from `config.toml` are `ConfigSettings { keymap, music_dir }` | `SourceOutcome`, `source_result` |
| applying a patch | `patched` (`TomlAppearance::patched`, kernel `AppearanceSettings::patched(patch)`) | `apply` |
| verbs, one meaning each | `transition` = machine step (message → `Cmd`); `execute` = driver runs an effect (IO); `Set*` = absolute command variant (`WindowColorsCmd::Set(ThemeName)`); `set_*` = method replacing one held value (`Painter::set_window_colors`); `patched` = value + patch → new value; `paint` = drawing into a buffer. Today's `apply_*` machines become `transition` (`ConfigDriver::apply_save_result` → `ConfigMessage::Saved`) | `apply` |
| spectrum | `spectrum_*` | `eq_*` |
| animation | `Animation`, `Cue`, `AnimationStage` | `Effect*` for animation |
| the subsystem only | `Config` (crate, `ConfigCmd`, `ConfigDriver`, `config.toml`) | `Config` / `File` as suffix of a raw or parsed shape |
| kernel scope | kernel holds domain state, messages, decisions and the `Machine`/`Driver` contract; formatting helpers used only by the view live in widgets; toast text and error text stay kernel data | view formatting in kernel |
| value a driver publishes to a cell | effect variant `Publish<Value>` (`PublishTheme`, `PublishAppearance`, `PublishCover`), sink field `publish_<value>` (decided 2026-10-04) | — |
| latest-value cell | `LatestSender`, `LatestReceiver`, built by `latest_channels` (frozen 2026-10-04) | — |
## 9. Banned words and patterns

| banned | source | check |
|---|---|---|
| `Ui` prefix on any name | memory 2026-09-16, `RETIRED_NAMES` | guard |
| `Cfg`, `Ctx` in a type name | `naming.rs` | guard |
| type suffixes `Props Tuning Sync Scratch Slices Info Type Kind Inputs Values Flags Params Options Data Manager Handler Helper Util Utils Wrapper Holder Draw Spec Slot` | `naming.rs`, `forbidden_names.rs`, review criteria | guard |
| type suffixes `State` (machine parts), `File`/`Config` (raw or parsed shapes), `XColors` other than `Colors` | decided 2026-10-02/03 | review |
| `Loop` suffix except `DriverLoop`, `MainLoop` (AppKit) and runtime's `EventLoop`; no per-driver loop | decided 2026-10-02/03 | guard |
| identifiers ending `Refused`, `Fault`, `Problem`, `Failure`, `Rejected` | `forbidden_names.rs`, decided 2026-10-02/03 | guard |
| words `Outcome`, `apply`, `Look`, `Adjust`, `Nudge`, `Subscription`; type prefix `Custom*`; `*Overlay` widgets | decided 2026-10-03 | guard |
| `*Request` outside kernel shell → core enums | decided 2026-10-03 | review |
| `Machine::Error`, machine error enums, `UpdateError`, `DriverEvent::Rejected` | decided 2026-10-03 | guard |
| `XParts` holding anything but `Model` borrows; one-slice `XView`; `Scene::*_view()` and `Scene::layout_parts()` getters; `Role::` inside painters | decided 2026-10-03 | review |
| `fn render*` other than ratatui trait methods (`Widget::render`, `StatefulWidget::render`) | decided 2026-10-03 | guard |
| a single named field in an enum variant; `type XResult = Result<…>` aliases | decided 2026-10-03 | guard |
| `fn sync_*`, `sync/` module dirs; `fn get_*` (a getter is the noun: `volume()`; `set_*` stays for a method replacing one held value, §8); predicates `should_ wants_ needs_` (use `is_ has_ can_`); `maybe`; `x_to_y` functions | `naming.rs`, `forbidden_names.rs` | guard |
| `compile`, `build_*`, `make_*`, `create_*`, `place_*`, `resolve_*` returning `Self` (use `new`, `with_*`, `from_*`) | `forbidden_names.rs` | guard |
| module files `reduce.rs compile.rs route.rs handle.rs process.rs dispatch.rs` | `forbidden_names.rs` | guard |
| abbreviations `vol proto cm hw dur tech bg msg pos err` and `_vol vol_ eq_ tech_ _proto _dur`; `draw*` (except `draw_pixmap`); allowed: only `Cmd`, `buf`, `px` | `naming.rs` | guard |
| parameter names `data info ctx cfg opts options idx tmp res val value handle item entry thing stuff params args props w h n i` | `naming.rs` | guard |
| every name in `RETIRED_NAMES` (`Skin`, `Footer`, `Notice*`, `Focus`, `KeyEffect`, `MediaEvent`, `AudioFault`, `Palette`, …) | `naming.rs` | guard |
| `#[allow]`, `#[expect]` outside tests; `super::` paths; glob imports; `macro_rules!` and own proc-macro crates; comments other than `SAFETY:` / `PROTOCOL:` / `GUARD:` one-liners | clippy, `imports.rs`, `macros.rs`, `comments.rs` | guard |
| `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!`, slice indexing in `src` | clippy | guard |
| `bool` parameters (a two-variant enum instead, with no methods); `bool` + `Option` pairs, `Option<Option<_>>`, `Vec` + `usize` pairs (`Cursor { index, len }` is a bounded index and allowed; its owner resets `len` whenever the list changes); `_ =>` on own enums | clippy, review criteria | guard |
| `Option<Vec/Box/Result>` unless empty and absent differ | memory 2026-10-01 | review |
| functions over 3 parameters, 60 lines or cognitive complexity 15; nesting over 3; files over 800 lines; test-only `src` files | clippy, `length.rs`, memory | guard |
| production code that only tests call; tests use the production API or assert on the data | user 2026-10-03 | review |

## 10. Off-convention rule

A name or shape this file does not cover (a new suffix, a new domain word, a second word for a concept listed here, a machine or driver form other than §3–§4) is not invented. Stop, report the name and where it is needed, and wait for a decision. Rows marked `decide` in the change list are such cases: do not pick a name while editing. `review`

## 11. Functions and data

1. Below the roots a function is pure: its output depends only on its arguments. `review`
2. Expression style: a value built from `match`/`if let`/iterator chains and `Option`/`Result` combinators; `let` over `let mut`. `mut` is for model and machine state; local builders are expressions (iterator chains, struct update). `review`
3. One concern per function; no magic values: numbers and glyphs are named constants or `impl Default` owned by the code that uses them. `review`
4. Illegal states are unrepresentable: fields that can contradict each other merge into one enum. `review`
5. A parameter struct only for a real value bundle, otherwise a method on the receiver; `bon` for four or more optional fields, never `.maybe_x(None)`. `review`
6. Data over code: themes, keymaps, presets are data files, not match arms. `review`
7. Resources are RAII: raw mode, alternate screen and threads are restored by a guard's `Drop`. `review`
8. Own traits are `Machine`, `Driver`, `Shell` (runtime↔binary seam), one narrow trait per hardware or OS source (`Watcher`), and a trait shared by three or more value types (`Bounded`, `Flag`); methods take and return data, static dispatch, never `dyn`; fakes plug in over the same real channels. `review`
9. Public API is minimal: `pub(crate)` unless a downstream crate needs it; no re-exports or shims for compatibility; delete, never deprecate. No extra abstraction, dependency or crate feature. `review`
10. Modules: no cycles inside a crate; leaves never import roots (no widgets module outside `screen` imports `crate::screen`) `guard` (`layering.rs`); a module's fan-in/fan-out stays within 3 × the crate median or the review names why. One responsibility per module; a second responsibility moves to its own module named for its noun. `review`

## 12. Events, frames and performance

1. Three event classes, one path each: a fact reaches `update` as a `Message` through the bounded mailbox; driver internals never leave the driver thread; a stream value (spectrum, decoded cover, reloaded theme, reloaded appearance) goes into a latest-value cell, never a queue. No message exists only because time passed. `review`
2. Cells are lock-free: `triple_buffer` for samples, atomics for scalars, `arc-swap` for large rare values. On a realtime path (audio callback, OS callback) no `Mutex`, allocation or blocking call; it only sets a flag or `try_send`s into a bounded(1) doorbell. `review`
3. Kernel timers (`Effect::After`) exist only for decisions (`Timer::Lookahead`, `Sleep`, `Toast`); a timer whose only purpose is to move pixels is a defect. `review`
4. `update` names each transition worth animating as `Effect::Animate(Cue)`; the shell plays the cue and never diffs the model to guess what changed. `review`
5. Frames only while something moves: every `frame_due` source is a pure function of (layout, anchor, now) that never slides; spectrum frames only while it is on screen and playing or decaying; with nothing moving the loop blocks with no deadline. `review`
6. Every channel that carries traffic is bounded; the loop never blocks on a driver (`try_send`; full → the command is dropped (`DropReason::Full`) and the port's congestion flag rises); a driver raises its flag before it blocks on a full mailbox; one episode (`Episode { Clear, Reported }` per port) yields exactly one `DriverEvent::Full` and one toast, and ends at the first loop iteration where the port's flag stays down. `review`
7. A message with no visual change: no paint, no allocation. A burst of keys or messages is drained as one batch and paints once. Spectrum frames never queue and never wake the loop. Idle: every thread blocked, 0 % CPU. `review`
8. Playlist rows borrow precomputed display text, no `String` per row per frame, `Arc::clone` over deep clones; pixel components are memoized by (input, generation) and re-encode only on change. `review`
9. Performance is read from the code in reviews; it is measured only when the user asks, never inside a step. `review`
10. Config lives under XDG; the working directory is never written. `review`

## 13. Tests

1. A private function is tested in `#[cfg(test)] mod tests` at the bottom of its file; a public contract in the crate's `tests/` tiers (`docs/testing.md`). `review`
2. `rstest` cases and `insta` snapshots, fixtures on disk; behaviour is covered, not lines; every snapshot is meaningful. `review`
3. A machine is tested as a table of (state, message) rows, refusals included: a refusal asserts `Err(Unhandled)` and the unchanged state. `review`
4. Runtime tests are thin, use real channels and need no sound device. A test that needs hardware is `#[ignore = "hardware: …"]` `guard` (`hardware.rs`); a contract test ignored for hardware is a defect in the seam. `review`
5. A refactor leaves snapshots unchanged except renamed identifiers; a behaviour change comes with a new snapshot and its reason. `review`
6. In test code `unwrap`, `expect`, `panic!`, indexing, `print!`, `dbg!` are allowed (`clippy.toml` `allow-*-in-tests`); structure is held to the production bar. `guard`
7. Agents commit after `cargo fmt`, clippy and the guards (`cargo test -p sifr-guards`); they run no test suite; the coordinator runs the gate (`scripts/gate.sh`). `review`

## 14. Reviews

A review cites rules as `§section.rule` and outputs one file of rows ranked H/M/L, each row `path:line`, the rule, the defect and the fix, no praise; then a task list for the fix agent, each task ≤ 30 minutes. A reviewer reads, never edits. Besides the `review` rules it checks: error paths and edge cases (empty library, zero-size terminal, missing file, bad config, device gone, panics); threads and channels (races, shutdown order, a driver that dies or never answers, blocking on a full or closed channel, signals, terminal state restored).

## 15. Open (decide with the user)

Frozen as today (2026-10-04), not reopened on this route:

- S4: `PlaybackRequest` carried inside driver data (`RemoteInput`, `MacosEvent::MediaKey`).
- S13: owner of volume (macOS vs app) and which effects stay relative.

Navidrome (not in this route):

- G1: network IO shape (request/response, retries, auth, pagination; effect or job).
- G2: where credentials and the server URL live.
- G3: streaming a remote track, and who resolves `TrackRef::Remote` to a source.
- G5: a library of several sources, `Revision` per source.
- G7: where widget constants live and where widget snapshots go.
