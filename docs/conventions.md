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

1. Layer map: kernel: nothing; audio, library, macos, config, remote: kernel; runtime: drivers, kernel, config; widgets: kernel; terminal: kernel, widgets; sifr: anything. `guard` (`layering.rs`)
2. kernel and widgets are pure: no IO, clock, threads, env, channels. `guard` (`purity.rs`)
3. Only the roots see a whole `Model`: the kernel router inside `update`, `startup`, `Scene::from_model`. Below them a function takes its slice or an `XParts`. `guard` (`demeter.rs`, `demeter_views.rs`)
4. One entry each: `update` has one call site in runtime, there is one key router, one `startup`; the paint path never calls `update`. `guard` (`dispatch.rs`) for the `update` call site and the paint path, `review` for the one key router and the one `startup`
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
| Cmd | kernel | `struct Cmd<E = Effect, M = Message> { effects: Vec<E>, messages: Vec<M> }`; build with `Cmd::none()`, `Cmd::effect(e)`, `Cmd::message(m)`, `From<Effect>`, `From<Cue>`, `FromIterator<E>`; join with `then` (appends, in order); read with `effects()` / `IntoIterator`, split with `into_parts()` (runtime reads a driver Cmd); a nested machine's effects lift with `map_effect`, its messages through `into_parts()` (`ConfigDriver::drive_watch` turns each `ConfigChange` into its own `Cmd`). No other combinator (`merge`, `chain`, `batch`, `and`) | `Cmd` = what a machine returned | guard |
| Effect | kernel | one variant per target: `Audio(AudioCmd)`, `Library(LibraryCmd)`, `Macos(MacosCmd)`, `Remote(RemoteCmd)`, `Config(ConfigCmd)`, `WindowColors(WindowColorsCmd)`, `Animate(Cue)`, `RollShuffle(usize)`, `After { delay, timer }`, `Restart(DriverName)`, `Quit` | `Effect` | review |
| XCmd | kernel | the order to one driver, carried inside `Effect` | `AudioCmd`, `LibraryCmd`, `MacosCmd`, `ConfigCmd`, `RemoteCmd` | guard |
| Event | kernel (types) | what a driver reports; `From<XEvent> for Message`; driver lifecycle is `DriverEvent { Died, Stopped, Full }` | `XEvent` | guard |
| Answer | kernel | runtime's reply to a kernel effect it ran: a `Message` variant named for the effect in the past tense (`Effect::RollShuffle` → `Message::ShuffleRolled`) | past tense of the effect | review |
| Request | kernel | what the shell asks the core; always a branch of `Message` | `XRequest` | guard |
| Message | kernel | the only input of `update`: `X(XRequest)`, `X(XEvent)`, answers, `Elapsed(Timer)`, `Driver { driver_name, event }`, `Key(KeyPress)`, … | `Message` | review |
| Driver | audio, macos, library, config, remote | the top machine of one external source (§4) | `AudioDriver`, `MacosDriver`, `LibraryDriver`, `ConfigDriver`, `RemoteDriver` | guard |
| DriverLoop | runtime | one generic loop, one thread per driver (§4); a driver effect reaches it through `LoopEffect { Execute, Run, After, Watch, Unwatch }` (decided 2026-10-04); runtime seeds `XMessage::Started` into the inbox at spawn through the `DriverLoop` field `message: Option<D::Message>`; the loop's private next-input enum is `LoopInput` (not `Wake`, reserved for the audio feeder's wake-up); the inputs `Spawners` hands each driver thread are `SpawnSetup`, the audio start closure `SpawnAudio`; an audio driver that stops before handing over its tap is `SpawnError::TapLost { driver_name }`; a value the driver publishes goes out through a closure sink `P: Fn(T)` the runtime passes in | `DriverLoop` | guard |
| Stream | runtime | a repeated input started by a driver effect (`LibraryWatchEffect::Watch(PathBuf)`, as Crux `stream_from_shell`); runtime owns it and feeds its items back as `XMessage`s. The word `Subscription` is not used | `FileStream` | guard |
| Job | driver crate (type), runtime (thread) | slow blocking work a driver hands to the runtime worker as `XEffect::Run(XJob)`; the result returns as an `XMessage`; stale by `Revision` (§4.7) | `AudioJob`, `LibraryJob`, `MacosJob`, `RemoteJob` | review |
| Error | every crate that can fail | §6 | `Error`, `<Type>Error` | guard |
| Scene | widgets | `Scene::from_model(&Model, ScenePresentation)`; the only widget code that sees `&Model`; no `*_view()` or `layout_parts()` getters | `Scene` | guard |
| View | widgets | read-model holding fields from two or more `Model` slices; borrowed fields, no state, no `&Model`; built only by `XView::from_scene(&Scene)` | `XView<'a>` | review |
| Widget | widgets | every type with `impl Widget`, overlays included; every widget type ends in `Widget` (`ToastWidget`, `CardWidget`, `TooSmallWidget`); built as in ratatui and ratcn: `XWidget::new(..)` takes what the widget cannot paint without (its `input`, then the `ActiveTheme` when it paints in theme colours), every optional knob is a consuming setter named after the field (`style(XStyle)`, `speed_chip(SpeedChip)`, …); fields are private, so there is never a struct literal outside its module and never a `builder()`; `input` is `&` one Model slice or one `XView` | `XWidget`, never `*Overlay` | guard |
| Style | widgets | a component's look; built only by `XStyle::from_theme(&ActiveTheme)`; an input beyond the theme rides on `ActiveTheme` through a builder (`with_progress_bar`); fields are semantic colours (`foreground`, `muted_foreground`, `background`, `border`, `accent`, …) | `XStyle` | review |
| Colors | widgets | only the theme palette | `Colors` | guard |
| raw TOML | config | every serde shape of a file or a section; each carries `#[serde(expecting = "…")]` in user words (`"a [cover] table"`), so a Rust name never reaches a toast | `Toml*` (`TomlTheme`, `TomlAppearance`, `TomlKeymap`, `TomlCard`, `TomlColors`, `TomlAudio`) | guard |
| parsed value | kernel, widgets | what inner code uses; parsed once at the boundary, never re-checked | bare noun (`Theme`, `Keymap`, `Appearance`) | review |
| user settings | kernel | values the user edits in a file or the settings overlay | `XSettings` (`Settings`, `AudioSettings`, `AppearanceSettings`) | review |
| config file id | kernel | `ConfigName { Config, Appearance, Theme(ThemeName) }` | `ConfigName` | review |
| read result | config | the outcome of reading and parsing one file at startup | `Parsed<T>` | review |
| test-table row | tests | one row of an `rstest` table | `XRow` | review |

## 3. Machines

1. Every part that reacts to messages is `impl Machine` with `transition` (Crux: every part is `update`). There is no second form; the word `apply` is not used. `review`
2. `transition` returns `Result<Cmd, Unhandled>` (the `Cmd` type as above), never `Option`, `Vec`, a bare effect enum or an `*Outcome`. `pub struct Unhandled;` (kernel, one for the whole workspace, no reason inside, as statig/XState "unhandled" and rust-fsm `TransitionImpossibleError`): a message with no transition in the current state returns `Err(Unhandled)` and leaves `self` untouched; `Ok(Cmd::none())` means handled with nothing to do outside. Runtime reads it: `Ok` repaints, `Err` does not repaint; the kernel `update` restores the dismissed toast and the clock before it answers `Err`, and a refused key that released a chord prefix answers `Ok`; a driver loop drops `Err(Unhandled)` silently (nothing to repaint); so do the kernel drain for a refused follow-up `Cmd` message and the runtime for a refused startup or answer delivery, the envelope frame loop and the painter. A kernel handler that also asks the player (an audio error, a lost output, a fired sleep timer, a speed step) answers the player's refusal with what it still did, the toast or the transport change (`Err(Unhandled) => Ok(raised)`), never with `Cmd::none()`. `machine::each_handled` answers a driver's `Cmds` batch as handled when one item is and refuses it only when every item is. The `refusal sites` block below holds every site as `<count> <path under crates/>: <the consuming line as rustfmt writes it>`; that block is the whole sanctioned list, and a change that moves or rewrites a site updates its line in the same commit. A handler takes every fallible step before its first mutation, or the steps after it cannot fail by their type (they return `Cmd` or nothing), so a refusal never leaves a half-applied model. No per-machine error enums. `review`

```
refusal sites
1 runtime/src/runtime.rs: Ok(()) | Err(Unhandled) => {}
1 runtime/src/runtime.rs: if let Ok(next) = self.update(current) {
1 runtime/src/driver.rs: let Ok(cmd) = driver.transition(message) else {
1 runtime/src/event_loop.rs: if self.runtime.deliver(message).is_ok() {
1 audio/src/deck/envelope.rs: Ok(()) | Err(Unhandled) => {}
1 sifr/src/shell/painter.rs: if let Ok(cmd) = self.window_colors_write.transition(message) {
1 kernel/src/update/mod.rs: let Some(cmd) = branch(model, message, clock).ok() else {
1 kernel/src/update/mod.rs: Err(refusal) => {
1 kernel/src/update/machine.rs: (Ok(handled), Err(Unhandled)) | (Err(Unhandled), Ok(handled)) => {
1 kernel/src/update/machine.rs: (Err(Unhandled), Err(Unhandled)) => Err(Unhandled),
1 kernel/src/update/audio.rs: Err(Unhandled) => Ok(raised),
1 kernel/src/update/audio.rs: Err(Unhandled) => recorded.map(|()| Cmd::none())?,
1 kernel/src/update/mod.rs: Err(Unhandled) => Ok(cleared),
1 kernel/src/update/playback.rs: Err(Unhandled) => Ok(stepped),
```

3. The input is an enum `XMessage` where X is the machine's type name (for an impl on `Option<T>` or `CursorOver<T>`, the noun of `T`; for a top driver machine, the subsystem: `AudioMessage`, `MacosMessage`, `LibraryMessage`, `ConfigMessage`), even with one variant, so a new input source is one new variant. Exception (Crux: no parallel enum that maps 1:1): when the machine's input would be identical to a shell `XRequest` enum, the machine takes that `XRequest` as its `Machine::Message` (`CursorOver<SearchQuery>` takes `SearchRequest`, `TextEntry<E>` takes `TextRequest`). The parameter is always `message`. `guard`
4. A message to self or parent is `Cmd::message(m)`; no follow-up or out-message types. DECIDED 2026-10-03 (as Crux `process_event`): kernel `update` drains every `Cmd` message itself, depth-first, in order, inside the same call, and returns only the collected effects, so a step and its follow-ups are atomic, no frame sees a half-applied model, and kernel tests see the final state through the production `update`. Depth over 8 is a programmer error: `debug_assert!`, and release stops the chain. `review`
5. The match is exhaustive, no `_ =>`. A variant move uses one `mem::replace`; `mem::take` on machine state is banned. `guard` (`_ =>`; `mem::take` of a `self` field inside `transition`), `review` (`mem::take` elsewhere)
6. A machine reads no clock. Kernel time arrives as `Moment`; driver commands arrive as `XMessage::Cmds(Cmds { cmds, at: Instant })` (kernel `pub struct Cmds<C> { cmds: Vec<C>, at: Instant }`; `DriverLoop` needs `D::Message: From<Cmds<C>>`) (`DriverLoop` reads the clock; time as data, as `crux_time`). `review`
7. A part is a plain noun naming what it works on, with no `State` suffix: `Engine`, `Hardware`, `Cover`, `ConfigWatch`, `CoverDecoding`. No two public types in the workspace share a name, except each crate's boundary `Error`. `guard`
8. Keys go to one key context: every binding names its `KeyContext`; an open overlay takes every key (a typed letter never reaches a global command), and with no overlay the router tries playlist, then global. `Esc` and `q` close every overlay through ordinary `Close` bindings, not router special cases (decided 2026-10-05). `review`
9. A machine on a realtime thread (cpal callback: `Envelope`) has `Effect = ()`: `transition(&mut self, message) -> Result<(), Unhandled>`, so it allocates nothing. `guard`

## 4. Drivers

Why `Driver`: same roles as OS drivers (request in, interrupt-driven events out, no driver-to-driver calls; DriverKit: the system owns queues and threads). Not `Actor`: drivers never message each other.

1. Each driver has one top machine `XDriver` that implements `Driver` (§4.5a). Its input `XMessage` has the variant `Cmds(Cmds<XCmd>)` plus one variant per own signal source (job results, stream items, callback input). `review`
2. `XDriver::transition` returns `Cmd<XEffect, XEvent>`: `effects` are actions, `messages` are reports to the kernel. An `XEffect` enum has no variant carrying an event. DECIDED 2026-10-03 (Crux `map_event`): `messages` always go one level up to the parent, at every level; a part inside a driver returns `Cmd<PartEffect, XMessage>` (`ConfigWatch` → `Cmd<ConfigWatchEffect, ConfigChange>`, which `ConfigDriver` lifts) and the driver lifts its effects with `map_effect` and its messages one by one after `into_parts()`; the top driver's parent is the kernel, so its messages are `XEvent`. A machine never messages itself; an IO answer comes back through `execute` (§4.3). No driver builds a kernel `Message`. `review`
3. The only impure step is `XDriver::execute(&mut self, effect: XEffect) -> Option<XMessage>`. `None` = fire-and-forget (Crux `Output = ()`); `Some(answer)` = the IO result, which `DriverLoop` feeds to `transition` at once, before the next inbox item (Crux `resolve`). The answer to effect `X` is the message variant named for it in the past tense (`Open` → `Opened`, `Save` → `Saved`, `Read` → `ReadDone`). Words `perform`, `handle`, `process`, `dispatch`, `apply` are banned for it. `guard`
4. Data a driver needs at start are fields of `XDriver`, not a `*Parts` bundle. `review`
5a. DECIDED 2026-10-03 (Crux: effects start everything, the shell owns the loop): kernel `pub trait Driver: Machine { type Effect; fn execute(&mut self, effect: Self::Effect) -> Option<Self::Message>; }` is the only driver trait; kernel `enum Driver` (which driver) becomes `DriverName` (as `OverlayName`, `ConfigName`). No per-driver hooks in `DriverLoop`: a deadline or debounce is an effect `After { delay, timer }` (as kernel `Effect::After`, Crux `notify_after`); watching files or any repeated input is an effect that starts a stream (`LibraryWatchEffect::Watch(PathBuf)`), whose items come back as `XMessage`s; shutdown is an ordinary kernel command sent before `Quit` (`ConfigCmd::Flush`), not a loop hook; `DriverLoop` drains the inbox and delivers all pending commands as one message `XMessage::Cmds(Cmds<XCmd>)`, so coalescing (keep the last volume) is pure machine logic tested by tables; `DriverLoop::spawn(start: impl FnOnce() -> D + Send)` builds the driver on its own thread (the cpal output stream is `!Send`). Rejected: Elm-style `subscriptions()` (a second mechanism beside effects). `review`
5. A driver opens no thread and no channel. `DriverLoop` (runtime) opens and closes every thread, channel, worker and stream: receive, read the clock, `transition`, `execute` each effect, send each event as a `Message` into `inbox`. There is no per-driver loop type. Threads a library opens inside itself (the cpal output stream, `notify` watcher) are excepted. `review`
5b. DECIDED 2026-10-03: a thread the OS or a library owns (AppKit main thread, CoreAudio listeners, cpal output) is a callback, never created or joined by us. Runtime opens a channel into the driver's inbox and hands its sender to whoever registers the callback; the callback only parses its input (§4.6) and sends an `XMessage`; every decision is in the driver machine on its own thread. The AppKit main loop `MainLoop` is started by runtime `host` (the name follows AppKit; allowed `Loop` types are in §9). `review`
6. Input from a callback that must answer synchronously (AppKit remote commands) is parsed at the boundary and decided in the machine: `RemoteInput::parse(trigger, event) -> Result<RemoteInput, RemoteInputError>`; a parse error answers `CommandFailed` at once, a parsed input is sent as `MacosMessage::Remote(input)` and answers `Success`. `review`
7. An `Effect` is what a machine asks to be done outside; it runs at once (`execute` for a driver, runtime for the kernel). A `Job` is slow work for a worker thread, carried as `Run(XJob)` (not `Queue`: queue is the play queue); its result returns later as an `XMessage`. A job in flight carries a `Revision`; a result whose `Revision` is stale is dropped. `review`
8. Nothing shared crosses a driver boundary: values cross as owned messages or through latest-value cells; no `Mutex`/`RwLock` handed out. A plain fn the composition root hands a driver (`library::cover::cover_bytes` for macos covers) is code, not shared state. `review`
9. Platform code lives in its own crate (`macos`), a `[target.'cfg(target_os = "…")'.dependencies]` entry, never a plain dependency; with one backend per target the choice is compile-time, no trait object. `review`
10. Adding a driver is this checklist, nothing else: a `DriverName` variant; its `runtime::registry` `DriverRow` (driver name, thread name, platform) and its `Supervision::standard` arm; its port in `Ports`; `XCmd` and the variant `Effect::X(XCmd)` with its interpreter arm; `XEvent` and `From<XEvent> for Message`; `XMessage` with `Cmds(Cmds<XCmd>)`; `impl Machine` and `impl Driver for XDriver`; `DriverLoop::spawn(start)` in wiring; a table test of `transition`. Wiring outside these places is a defect. `review`

## 5. Effects and enum variants

1. Variant shape by field count: no data → unit (`Play`); one field → tuple (`Seek(Duration)`); two or more → named fields (`After { delay, timer }`). A single named field is banned, with two exceptions: the context field of an error variant (`SpawnError::TapLost { driver_name }`, as thiserror) and a row count with no type of its own (`CursorBy { rows: i32 }`). A unit goes into the type, not the field name: time is `Duration` (`NotAscending(Duration)`; a signed step is `{ direction: Direction, by: Duration }`); a count is a tuple, the variant name says what it counts (`RollShuffle(usize)`). `guard`
2. An effect carries only the data its action needs. A compound action is several effects in one `Cmd`, in order (a speed step is `AudioCmd::SetSpeed(speed)` then `MacosCmd::SetSpeed(speed)`), as Crux `Command::all`/`then`. `review`
3. An effect is a whole desired state or carries a `Revision`; effects are absolute (`SetVolume(…)`), so a repeat is harmless. Stale timers are dropped by `Revision`; `Effect::After { delay, timer }` stays (as `crux_time` `NotifyAfter`). `review`
4. A newtype exists when the value has a rule (range, invariant: `Speed(f32)`), a unit (`Cells`, `Pixels`) or an index space (`TrackIndex`, `ViewIndex`); a bare integer in geometry is a defect; every `as` cast sits in a named conversion: a unit conversion is a method on the newtype that owns the unit (`Frames::duration(rate)`), a plain numeric cast is a fn named by its result (`bin_index`), never `x_to_y`. `review`
5. Use std types where they say it: `Result<..>` and `Option<..>` written out; no `Result` aliases (`type XResult = …`). `guard`

## 6. Errors

1. Every crate that can fail has one boundary `Error`. Machines and `update` return `Unhandled` (§3.2), not an error type. `guard`
2. An error of parsing or validating one value is `<Type>Error` (`ThemeNameError`, `TimecodeError`, `RemoteInputError`); its out-of-range case is the variant `OutOfRange` with the offending value and the bound, the value field named for its kind: `{ value, max }` for a count (`TimecodeError`), `{ duration, max }` for a `Duration` (`CrossfadeError`), plus `min` when the floor is not zero (`SleepPresetsError`). A panic payload is not carried: `DriverError::Panicked` is a unit variant; the payload text is dropped until runtime devtools exist (§6.4). `guard`
3. `thiserror` enum, `#[error]` user text on every variant, `#[source]` chains, context fields, no `String` payloads (config reload: §8; `ConfigError: PartialEq` so a toast shows only for a new error; user text comes from `Display` when the toast is built), no `Box<dyn Error>`, no `anyhow`, `From` only along a real crate edge. No `let _ =` anywhere, tests included (a value is handled, propagated, asserted or turned into an event), no `unwrap_or_default` hiding an error. `guard` (`conventions.rs` `let_underscore` and `ok_discard`, clippy `disallowed-methods`)
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
| `Row` | settings / registry / catalog / test-table row | `SettingRow`, `DriverRow`, `CatalogRow` |
| `Patch` | partial change to a stored value; every field an `Option`, built by struct update `XPatch { field: Some(v), ..XPatch::default() }`, no builder crate | `ConfigPatch`, `AppearancePatch` |
| `Index` | position newtype in one index space | `TrackIndex`, `ViewIndex` |

## 8. Domain words

Editor-shaped concepts take Zed's word (`Theme`, `Keymap`, `Workspace`, `Toast`; Zed's modal is our `Overlay`). The "not" column is held by `RETIRED_NAMES` (`guard`); picking the word for a concept is `review`.

| concept | the word | not |
|---|---|---|
| track | `Track`, `Arc<Track>`; value `track` | `song`, `song_title` |
| track identity everywhere (queue, favorites, history, m3u) | `TrackSource` (`enum TrackSource { Local(PathBuf), Server { server_name, server_track_id } }`); value `track_source`; field `Track.source` (the type takes the word its field already has); `Local` keeps today's path behaviour: no normalisation, same path text on disk, lookup through a `TrackSource` to index map, a dangling ref is skipped on load; `Server` names a track by its server and the server's id, never by a URL | `TrackRef`, `track_ref`, `track` for it, `source` alone outside `Track`, a path or an index as identity, `TrackRef::Remote` |
| a music server sifr knows | `Server { account, server_status }` (domain::server) in `Model.servers`; its state `ServerStatus { Connecting, Online(Session), Offline(RemoteError) }`; value `server` | — |
| who logs in where, without the password | `Account { server_name, endpoint, user_name }`, kept in `config.toml` `[[server]]` tables; value `account` | — |
| the password a connect uses | `Credential { Typed(Secret), Stored }`: typed just now, or the one in the Keychain (service "sifr", account "user@host"); value `credential` | — |
| the signed auth query after a connect | `Session { endpoint, query }`; its `Debug` is redacted; the Model keeps it, never the password; value `session` | — |
| a typed password in flight | `Secret`, only inside `Credential::Typed`; never stored in the Model | — |
| which catalog the browser shows | `CatalogName { Local, Server(ServerName) }`; value `catalog_name` | — |
| a server's browsed listings | `Catalog { server_name, albums_level, album_level }` and `BrowseLevel { listing, catalog_rows, cursor, paging }` (domain::catalog), in `Model.catalogs`; values `catalog`, `albums_level` | — |
| one row of a server listing | `CatalogRow { Album(ServerAlbum), Track(Arc<Track>) }` (domain::track); value `catalog_rows` | — |
| what audio opens | `Media { Local(PathBuf), Growing(GrowingMedia) }` (`TrackLoad.media`); `Growing` is a download still being written, read up to its safe byte bound; value `media` | — |
| one ordered download chunk | `MediaFetch { server_name, server_track_id, cache_key, session, first_byte, revision }` in `RemoteCmd::Fetch` (the playing track) and `RemoteCmd::Prefetch` (the next one); value `media_fetch` | — |
| a download the kernel drives | `Download { media_fetch, fetched }` in `Model.downloads`; value `download` | — |
| a play to report to its server | `PlayReport { server_name, server_track_id, scrobble }` in `Model.play_reports`, flushed at quit and restored at start; value `play_report` | — |
| what a play report says | `Scrobble { NowPlaying, Played(Moment) }`; value `scrobble` | — |
| track number tag | `Tags.track_number` | `track` for it |
| time into the track | `Duration`; value `position`; `offset` only for a position at an anchor (`Playhead.offset`), `target` for a seek destination, `by` for a relative step | `at`, `offset` for anything else |
| track length | `Duration`; value `duration` | `total`, `decoded` for a length |
| playhead anchor | `Playhead`; value `playhead` | `head` |
| preloaded next track (kernel record) | `Option<Arc<Track>>` in `Player::Playing`; value `preloaded` | `Preload`, `Requested`, `seek_reset`, `Preload::Queued`, `Preload::Stale`, `Preload::Requested` |
| player state | `Player`; value `player` | — |
| play queue | `Model.queue: Vec<TrackSource>`; value `queue` | `queued` for anything else |
| queue number on a row | `QueueNumber`; value `queue_number` | `QueuePosition`, `position` |
| track position in the library, the shown list | `TrackIndex`, `ViewIndex`; value `index` (`view_index`, `track_index` when both are in scope) | `PlaylistIndex`, `QueueIndex`, `row` for a `ViewIndex`, `browse_selected`, `selected_line`, `cursor_index`, bare `usize`, `StatusLineView.position`, `PlaylistSlot`, `ViewRow` |
| row of a settings or history overlay | `RowIndex`, `SettingRow`; value `row`; cursor `selected` | `current`, `selected: usize`, `SettingKind` |
| repeat | `RepeatMode`; value `repeat_mode` | `repeat` |
| shuffle, play order | `PlayOrder`, `Shuffle`; values `play_order`, `shuffle` | `PlayOrder::Shuffle`, `Disabled`, `Enabled` |
| switch, visibility and flag enums | `On`, `Off`; `Shown`, `Hidden`; `Yes`, `No` (`Playing`, `Selected`, `Favorite`, `DeviceDefault`); value the type word | `Enabled`, `Disabled`, `Marked`, `Unmarked`, `Default`, `Named`, `Other` |
| volume | `Percent`; value `volume` | `VolumeMode`, `VolumeConfig` |
| speed | `Speed`; value `speed` | — |
| trash a file | `Overlay::ConfirmTrash`, `OverlayName::ConfirmTrash`, `Cue::TrackTrashed`, `KeyContext::ConfirmTrash`, `LibraryCmd::Trash`; `Action::Delete` keeps its word; its keymap key is "delete" | `ConfirmDelete`, `DeleteCandidate`, `TrackDeleted`, `Action::Trash` |
| replay-gain tag value | `Decibels`; value `decibels` | `gain` for a `Decibels`, `replay_gain` for it, `TrackLoad.replay_gain` |
| linear gain factor in audio | `Gain`; value `gain`; a replay-gain tag value is `Decibels` in kernel, converted to `Gain` in audio | bare `f32` |
| listed device default flag | `ListedDevice { name, default: DeviceDefault }`; `DeviceDefault { Yes, No }` | `Default`, `Named`, `Other` |
| device that opened | `DeviceChoice`, `DeviceOpened`; values `device_choice`, `device_opened`; field `choice` only in `Device*` owners | `opened`, `reopened` |
| output loss error | `OutputError`; value `error` | `StreamError`, `Stream*`, `kind`, `AudioFault` |
| progress time setting | `ProgressTime`, `AppearanceField::ProgressTime`, `PROGRESS_TIMES`; value `progress_time` | `ProgressRemaining`, `PROGRESS_STYLES` |
| progress bar look | `ProgressBar`; value `progress_bar` | `progress` for it, `BarRoles`, `BarPalette`, `ProgressImageKey`, `ProgressImageSpec`, `ProgressParts`, `ProgressLine` |
| theme choice | `ThemeChoice`; value `choice` in `Themes`, `theme_choice` elsewhere | `selected` for it |
| theme input colours before derivation | `ThemeBase`; value `theme_base` | `ThemeSeed`, `seed` (`Seed` = DriverLoop seed), `base`, `raw` |
| toast and level | `Toast`, `ToastLevel`; values `toast`, `level` | `Notice`, `kind`, `SaveBannerKind`, `SaveBanner`, `SaveOutcome`, `NoticeLevel`, `NoticeLifetime`, `NoticeUpdate`, `NoticeOnScreen`, `ToastUpdate`, `NoticeDurations` |
| staleness of a timer or job answer | `Revision`, `Freshness`; values `revision`, `freshness` | `Reply`, `reply` |
| config file id | `ConfigName`; value `name` | `file` |
| text typed into a prompt | `TextEntry`, `SearchQuery`; value `text_entry` | `typed`, `entry` |
| history entry | `HistoryEntry { track_source, played_at }`; value `history_entry` | `played`, `entry` as a parameter, `at`, `EntryRow.entry`, `HistoryMessage::First`, `HistoryMessage::Last`, `HistoryLine` |
| sort key | `SortKey`; value `sort_key` | `sort`, `key` |
| key bindings | `Keymap`, `KeyBinding`, `KeymapOverrides`, `KeyOverride`, `Chord`, `KeyContext`; values `binding`, `keymap_overrides`, `key_override`; a binding maps to a `Message`; an unbound `Char` in a text `KeyContext` is typed input in lookup (Zed) | `Keys`, `keys`, `Focus`, `KeyEffect`, `KeyOutcome`, `CharSink`, `AnyChar`, `overrides`, `slot`, `rebind`, `BindingSource`, `source`, `KeysConfig`, `KeysError`, `KeyEntry`, `BindingAction`, `FocusRow` |
| chord prefix | `ChordPrefix`; value `chord_prefix` | `prefix` alone |
| scan state | `ScanStatus`; values `scan_status`, `scanning_label` | `ScanProgress`, `scan`, `status` |
| tag pass | `Tagging`; value `tagging` | `Tagging::Read` |
| driver restart policy | `Supervision`; value `supervision` | `strategy` |
| driver status | `DriverStatus`, `DriverStatusMessage`; value `error` | `failure` |
| error inside an error | `IoError`, `DecodeError`; value `source` as a field of an error type, `error` everywhere else | `kind`, `err`, `failure`, `error` for the cause field of an error type |
| parser text shown to the user | `Diagnostic`, built only by `Diagnostic::from_error(&impl Error)` in the crate that owns the parser; kernel `ConfigError::Parse(Diagnostic)`; value `diagnostic` where named | `reason`, `detail: String` |
| music dir | `PathBuf`; value `music_dir` | `target`, `source_dir` |
| config paths | `PathBuf`; values `config_path`, `appearance_path`, `themes_dir` | `config`, `appearance`, `themes` for paths |
| raw TOML text | `String`; value `text` | `source`, `raw`, `existing` |
| laid-out cover side | `Pixels`; value `side` (`cover_side`) | `size`, `vinyl_size`, `size_px` |
| shell to core: what the user asks | `*Request`; value `request` | `playback_request`, `browse_request`, private `Input::Key`, `Ui`, `UiRequest`, `UiPreset`, `UiMessage` |
| driver fact | `*Event`; value `event` | `event` for `KeyEvent` and `ShellInput` |
| machine input | `*Message`; value `message` | `told`, `SearchMessage` for the search machine (it takes `SearchRequest`), `PlaybackMessage`, `QueueMessage`, `LoadedMessage`, `SettingsRowMessage`, `TextMessage`, `JumpMessage` |
| driver order and a batch of them | `*Cmd`, `Cmds`; values `cmd`, `cmds`, `cmd_sender`, `cmd_receiver` | `command`, `commands`, `batch`, `EngineCmd`, `BatchFlags` |
| track load order | `TrackLoad`; value `track_load` | `load`, `pending`, `requested`, `request` |
| engine track record | `LoadedTrack`; roles `current`, `incoming`, `outgoing` | `CurrentTrack`, `preload`, `track` for the incoming one, `primary`, `swap_primary`, `retire_primary` |
| crossfade gain pair of one engine track | `Fader { control }`; values `incoming_fader`, `outgoing_fader` | `Slot` |
| decoded audio | `DecodedTrack { revision, decoder }`; value `decoded_track` | `TrackSource` for it, `source` |
| next-track preparation | `PreloadMode`; value `preload_mode` | `PreloadKind`, `Preload`, `mode` outside `Deck`, `ExpectPreload`, `expect_preload`, `Envelopes.gapless` |
| crossfade start | `EngineEffect::SetFadeStart`, `EngineMessage::FadeStartReached`, `Signals::FADE_START`; value `fade_start` | `cue` for it, `Cued`, `CUED`, `Arm`, `arm_cue`, `armed`, `rearm`, `EngineEffect::Cue`, `crossfade_cue`, `cue_set`, `recue` |
| job results of audio | `AudioMessage::Decoded`, `AudioMessage::Preloaded`, `AudioMessage::DevicesListed` | `DeckEvent::Decoded`, `DeckEvent::Preloaded`, `DeckEvent::DevicesListed` |
| job staleness | `JobRevisions`, `ExecutedRevisions`; value `revisions` | `ExecutedRevisions.incoming` |
| callback channel | `callback_sender`, `callback_receiver`, `WaitSource::Callback` | `heard`, `Heard`, `HEARD`, `callbacks`, `deck_sender`, `MacosChannel.sender`, `MacosChannel.receiver` |
| kernel inbox | `inbox` (send), `inbox_receiver` (receive) | `mailbox`, `messages`, `arrivals`, `sender_index`, `sender`, `report_sender`, `receiver`, `queued`, `inbox` for the 3 other queues |
| driver command queue | `cmd_sender`, `cmd_receiver` | `commands`, `command_inbox`, `inbox` |
| job queue | `job_sender`, `job_receiver` | `jobs`, `inbox`, `Jobs` |
| job runner | `run_job` | `Jobs { run }` |
| loop input | `LoopInput::Message` | `LoopInput::Heard` |
| full-inbox flag of a port | `Congestion`; value `congestion`; the event stays `DriverEvent::Full` | `full`, `Congested` |
| latest-value cells | `LatestSenders`, `LatestReceivers`, `LatestSender`, `LatestReceiver`, built by `latest_channels`; values `latest_senders`, `latest_receivers`, `theme_sender`, `appearance_sender` | `writers`, `cells`, `latest` |
| doorbell of the cells | `doorbell`, `Arrival::Doorbell` | `notified`, `Notified`, `notify`, `notifier`, `notices` |
| spectrum feed | `SpectrumTap`; values `spectrum_tap`, `bins`; names `spectrum_*` | `tap`, `eq_*`, `spectrum` for a `Handoff`, `RingBuf`, `MeterKey`, `MeterPlan` |
| file stream item | `Changed`; value `changed` | `item` |
| input the shell feeds runtime | `ShellInput`; value `input` | `ShellEvent`, `event` |
| repaint cause | `RepaintCause`; value `cause` | `source` |
| cover job | `CoverJob`; value `cover_job`; field `job` only in `Cover*` owners | `job` in `LibraryJob::Cover`, `cover` |
| wanted cover | `Option<CoverJob>`; value `wanted` | `asked`, `ask` |
| passed-over error | `Option<Error>`; value `skipped` | `first_error` |
| audio file extensions the scanner accepts | `&[&str]`; value `audio_extensions` | `decodable`, `is_decodable` |
| macOS hardware listeners | `HardwareListeners`, `Listener`, `MacosEffect::Listen`, `MacosMessage::Listened`, `MacosError::Listen`; values `listeners`, `listener` | `HardwareWatch`, `Watched`, `notify`, `MacosMessage::Listening`, `Watching`, `WatchedRefused`, `MediaWatch`, `WatchHandle` |
| now playing panel | `NowPlaying`, `MacosEffect::ShowNowPlaying`; value `now_playing` | `Publish`, `shown` |
| crossterm key event | `KeyEvent`; value `key_event` | `event` for it |
| terminal query result | `Option<Picker>` from `query`; value `picker` | `QueryAnswer`, `probe_answer`, `probed`, `query_answer` |
| terminal teardown | `Restoration`; value `restoration` | `RawMode`, `RawModeDisabled` |
| terminal app | `TerminalApp`; value `app` | `detect`, `Brand` |
| window colours order | `WindowColorsCmd`; value `cmd` | `command` |
| paint clock | `PaintClock`; value `paint_clock` | `first_paint`, `last_paint` |
| card cover | `CardCover`; value `card_cover` | `CoverArt` (library's), `cover_art`, `art` |
| cover image | `CoverImage`; value `cover_image` | `DecodedCover`, `decoded`, `cover`, `CoverPixels` |
| cover mode | `CoverMode`; value `cover_mode` (key `cover_mode`) | `CoverStyle`, `style`, `active`, `mode`, `PixelCoverStyle`, `RendererMode` |
| pixel path | `PixelPath`; value `pixel_path` | `detected` |
| time since first paint | `Duration`; value `since_first_paint` (`Workspace.clock` keeps its word) | `clock` for the shell clock |
| vinyl style | `VinylStyle`; value `style` in `Vinyl*`, `vinyl_style` elsewhere | `colors` for it |
| playback flag | kernel `Playback`; value `playback` | `playing` for it |
| playing row | `Option<ViewIndex>`; value `playing_index` | `playing` for it |
| appearance settings in widgets | `AppearanceSettings`; value `appearance_settings` | `appearance`, `settings` for it |
| status line read-model | `StatusLineView`; value `status_line` | `status`, `scan`, `position`, `total` |
| library loading state | `LibraryStatus` | `LibraryLoad`, `library_loading` |
| toast widget | `ToastWidget` | `toaster` |
| table cursor | `TableState` | `table_rows` |
| search matches | `SearchQuery.matches: Vec<ViewIndex>`; value `matches`; their count is `matches.len()` | `matches` for a count |
| key-hint chips | `KeyHintChords`, `Chip`; value `chip` | `keys`, `Chip.key` |
| speed chip | `SpeedChip`; value `speed_chip` | `mode`, `indicator_*` |
| format chips | `FormatChips`; value `format_chips` | `visibility`, `time_chip_*` |
| displayed track | `Option<&Arc<Track>>`; value `displayed_track` | `current` for it |
| scrollbar | `Scrollbar`; value `scrollbar` | `bar` |
| bar fill | `BarFill`; value `bar_fill` | `spec` |
| HUD progress | `HudProgress`; value `hud_progress` | `HudProgressRow`, `input`, `Row` suffix |
| breakpoints | `Breakpoints`; value `breakpoints` | `layout` for it |
| layout mode | `LayoutMode`; value `layout_mode` | `mode` for it |
| theme colours | `Colors`; value `colors` | `text` for a colour, `accent2`, `spectrum[2]` for the alert colour |
| modal geometry | `ScrollAreas`, `CellSize`, `ModalSize::FullWidth`; values `areas`, `body`, `content_rows` | `content_lines`, `content`, `PlacedSize`, `FrameWidth`, `modal_frame`, `ModalFrame`, `ModalColors`, `ModalChromeColors` |
| help rows | `HelpRow`; value `rows` | `bindings` for it, `KeyGroup`, `Group` |
| time text | module `time_text`; values `duration_text`, `elapsed_text`, `relative_time_text` | `format_time`, `elapsed_of`, `relative_time`, `clock_text`, module `clock`, `time`, `elapsed_total` |
| milkdrop stamp | `MilkdropStamp`; value `stamp` | `MilkdropTick`, `tick`, `WarpParams`, `InjectParams` |
| screen wash: a theme change fades every cell at once from the colours on screen in a quick fade | `screen_wash`; the colours on screen `PaintedCell` (`fg`, `bg`), value `painted_cells`, the start `wash_from` | `THEME_WASH_GRADIENT_CELLS` |
| scatter animation | `scatter_burst` | `delete_*` |
| animation inputs | `CellFilter`; value `cell_filter` | `guard`, `duration` for them |
| who paused playback | `PausedBy` | — |
| modal surface in the core | `Overlay`, `OverlayName`; render frame and geometry `Modal*` | `OverlayKind`, `OverlayScreen`, `Pane*` (only the `playlist::pane` module) |
| bottom key line | `KeyHints` (`key_hints`) | `Footer`, `SettingsHintLabels`, `FooterContent` |
| small label | chip (`format_chips`, `speed_chip`) | badge, `tech_chips`, `TechChips`, `TechChipColors` |
| preload-due / A-B-end timer | `Lookahead` (`Timer::Lookahead`) | `Mark` |
| A-B point | `AbMark` (`AbLoop::mark`) | — |
| render part of `sifr-ui.toml` | kernel `domain::appearance::Appearance` (`CoverCells`, `Breakpoints`, `ProgressBar`; built by `TomlAppearance::to_appearance`) | `Look`, `Custom*`, `UiOptions`, `[ui]` |
| choices the settings overlay edits | kernel `AppearanceSettings` (field `Settings.appearance`) | `SettingsValues`, `SettingsReadout`, `AppearanceSetting` |
| step a setting / volume / speed | `Step` + `Direction { Next, Previous }` (`Message::Step { row, direction }`, `StepVolume(Direction)`); size is a constant beside the value (`VOLUME_STEP = 5`) | `Adjust`, `Nudge`, `steps: i8`, `Adjusted` |
| absolute input (remote, IPC, macOS) | `Set*(value)` (`SetVolume`, as cliamp) | — |
| colour scheme / palette | `Theme`, `Colors` | `Palette`, `Skin`, `ActiveSkin` |
| terminal window bg/fg | `WindowColors` | `WindowTint` |
| replay gain | `ReplayGain` | `Replaygain` |
| seek by tenths | `SeekTenths` | `SeekFraction` |
| the audio feeder's wake-up of the driver | `Wake { Sent, Pending }`, fn `wake`; job staleness is a `Revision`, as timers | `Unsent`, `ring`, `Ticket` |
| frame pipeline | `FrameLayout`, `Scene`, `XWidget::render` (ratatui trait methods only), `paint*` (own fns writing into `Buffer`/`Canvas`/`Pixmap`) | `draw`, `render` for own fns or for building, `DrawnRows`, `FrameInputs`, `FrameRenderInputs` |
| values `Scene` takes beside the `Model` (theme, colour depth, bindings, …) | `ScenePresentation` | — |
| time passed into the kernel | `Moment` | — |
| first model | `startup`, `Startup` | init, `boot`, `Boot` |
| everything the binary reads before the runtime starts (first model, paths, theme) | `Launch { startup, paths, theme }`, `launch()` (decided 2026-10-04) | `Boot`, `Look` |
| terminal geometry unit | kernel `domain::geometry::{Cells(u16), Pixels(u32)}` (columns and rows alike; pixel sizes), imported by widgets, library and sifr (decided 2026-10-04, moved from widgets/library so `Appearance` and `visible_rows` can use them); ratatui `Rect`/`u16` stay at the ratatui boundary only | bare `u16`/`usize`/`u32` sizes |
| index into the sleep presets | `PresetIndex` (`Index` row) | `preset_index: usize`, `ResizeSleepCursor` |
| macOS `OSStatus` code | `OsStatus(i32)` | bare `i32` |
| progress bar's unfilled part | `groove` | `track` (collides with `Track`) |
| bad colour text in appearance | `ColorError::Malformed(Diagnostic)` | `input: String` |
| overlay content messages inside `OverlayMessage` | `OverlayContentMessage` | `InnerMessage` |
| default key bindings | Rust tables in kernel `update/keymap` (data in code by decision 2026-10-04) | embedded TOML |
| a child module's message handler | `update` (Elm), beside the root `update` | `step`, `handle` |
| audio engine with no device open | `EngineState::Closed`, its input `ClosedMessage` | `Muted` (reads as volume mute) |
| deck's wake-up signal, sent by the feeder | `DeckEvent::Woke(Revision)`, fn `wake` | `Track(Revision)`, `notify` |
| macOS main-loop parts | `MainLoopStop`, `NowPlaying`, `NowPlayingClock`; `HardwarePoll`, `ArtworkReader` keep their names | `LoopStopper`, `Panel`, `PanelClock`, `MediaWorker`, `MediaResult`, `MediaEvent` |
| terminal app identity, pixel protocol | `TerminalApp`; the pixel protocol is ratatui-image's `ProtocolType`; `Capabilities::from_environment`; `CoverPainter` | `Brand`, `Protocol { Kgp, Iip, Probe }`, `before_probe`, `CoverRenderer`, `CoverPlan`, `CoverAction`, `CoverPaint`, `CoverDraw`, `FramePrepaint`, `DeferredDraw` |
| config watch sub-machine step inside `ConfigDriver` | `drive_watch`; never-read file state `Seen::Unread` | `drive`, `Seen::Never`, `HotReloadPoll`, `WatcherPollTiming` |
| job coalescing in `DriverLoop` (keep the newest job per kind, run in `Ord` order) | allowed `DriverLoop` duty (scheduling, not a decision about the result; staleness stays `Revision` in the machine) (decided 2026-10-04) | — |
| discarding an `io::Result` | allowed only inside `Drop` and the panic hook, as `drop(result)` (nowhere to report) | anywhere else; `.ok();` |
| library disk orders inside `LibraryEffect::Execute` | `DiskCmd` | `LibraryCmd` reused |
| named `Rect`s of one `FrameLayout` part | suffix `*Areas` (`ModalAreas`, `PlaylistAreas`, `OverlayAreas`) | `Rects`, `Regions` |
| read-models `KeyHintsContent`, `OverlayContent` | `KeyHintsView`, `OverlayView` (View row) | `Content` suffix |
| sifr's values beside the Model for `ScenePresentation` | `ShellPresentation` | `Presentation` |
| CPU-parallel work inside one job (tag reading) | allowed: `thread::scope` inside a job body, joined before the job returns (decided 2026-10-04) | detached threads in jobs |
| turning the raw `TomlTheme` into the widgets `Theme` | the shell (sifr) does it: `Theme` is a widgets type and config sits below widgets; config publishes `TomlTheme` (decided 2026-10-04) | config depending on widgets |
| which cover to decode and at what size | the kernel decides: the shell reports the laid-out cover side through `Message::Viewport` (`Pixels`), kernel emits `Effect::Library(LibraryCmd::DecodeCover(CoverJob))` on track or side change; no shell→driver side channel (decided 2026-10-04) | paint path sending `LibraryMessage::Cover` |
| config reloaded | kernel `ConfigReload { name: ConfigName, result: Result<(), ConfigError> }` (§6.3), `config_reloaded`; the per-file errors held by the workspace are `ConfigErrors`, field `config_errors`; startup shows one toast with the first error and records the rest; the live values parsed from `config.toml` are `ConfigSettings { keymap_overrides, music_dir }` | `SourceOutcome`, `source_result` |
| applying a patch | `patched` (`TomlAppearance::patched`, kernel `AppearanceSettings::patched(patch)`) | `apply` |
| verbs, one meaning each | `transition` = machine step (message → `Cmd`); `execute` = driver runs an effect (IO); `Set*` = absolute command variant (`WindowColorsCmd::Set(ThemeName)`); `set_*` = method replacing one held value (`Painter::set_window_colors`); `patched` = value + patch → new value; `paint` = drawing into a buffer. Today's `apply_*` machines became `transition` | `apply` |
| animation | `Animation`, `Cue`, `AnimationStage` | `Effect*` for animation |
| the subsystem only | `Config` (crate, `ConfigCmd`, `ConfigDriver`, `config.toml`) | `Config` / `File` as suffix of a raw or parsed shape |
| kernel scope | kernel holds domain state, messages, decisions and the `Machine`/`Driver` contract; formatting helpers used only by the view live in widgets; toast text and error text stay kernel data | view formatting in kernel |
| value a driver publishes to a cell | effect variants `PublishTheme`, `PublishAppearance` (Publish plus the type of the value), sink fields `publish_theme`, `publish_appearance`, `publish_cover` (decided 2026-10-04) | — |

## 9. Banned words and patterns

| banned | source | check |
|---|---|---|
| `Ui` prefix on any name | memory 2026-09-16, `RETIRED_NAMES` | guard |
| `Cfg`, `Ctx` in a type name | `naming.rs` | guard |
| type suffixes `Props Tuning Sync Scratch Slices Info Type Kind Inputs Values Flags Params Options Data Manager Handler Helper Util Utils Wrapper Holder Draw Spec Slot` | `naming.rs`, `forbidden_names.rs`, review criteria | guard |
| type suffixes `State` (machine parts), `File`/`Config` (raw or parsed shapes), `XColors` other than `Colors` | decided 2026-10-02/03 | review |
| `Loop` suffix except `DriverLoop`, `MainLoop` (AppKit), runtime's `EventLoop` and the kernel's `AbLoop`; no per-driver loop | decided 2026-10-02/03 | guard |
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
| `..` closing a struct or tuple-struct pattern of a workspace type that binds a field in a `let` without `else`, a fn or closure parameter or a `for` pattern, outside `mod tests` (name each field, an ignored one as `field: _`; `X { .. }` that binds nothing stays; a `match` arm, `if let`, `while let` and `let … else` may close the pattern with `..`) | P39, P277, `rest_patterns.rs` | guard |
| an arm made only of wildcards in a tuple (`(_, _) =>`) over own enums, outside `mod tests` | P39, `wildcard_arms.rs` | guard |
| an arm whose `\|` alternation holds `_` or an all-wildcard tuple (`X \| _ =>`, `(_, Key::Up) \| (_, _) =>`) over own enums, outside `mod tests` | P41, `wildcard_arms.rs` | guard |
| `Duration::from_mins`, `from_hours`, `from_days` over an argument that is not a literal or a const, outside `mod tests` (they panic on overflow; use `from_secs` with a saturating product) | P41, `durations.rs` | guard |
| `Select::new()` in `crates/runtime/src` outside `mod tests` (a wait is `select_biased!` with a `default(timeout)` arm) | P39, `select_waits.rs` | guard |
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
2. Cells are lock-free: `triple_buffer` for samples and envelope orders, atomics for scalars, `arc-swap` for large rare values. On a realtime path (audio callback, OS callback) no `Mutex`, allocation, free or blocking call. The audio render callback `Mixer::mix` sends no signal: it takes decoded chunks, mixer orders and returns empty chunks and retired voices through wait-free rtrb rings, reads envelope orders from `triple_buffer` cells and leaves position and flags in atomics; the feeder, a `DriverLoop` job paced by the driver, refills the chunks and wakes the driver. An OS callback only sets a flag or `try_send`s into a bounded(1) doorbell. `Mixer::mix` and `FeedSource::read` carry `#[sanitize(realtime = "nonblocking")]`, and `scripts/rtsan.sh` runs the audio tests under RTSan. `review`
3. Kernel timers (`Effect::After`) exist only for decisions (`Timer::Lookahead`, `Sleep`, `Toast`); a timer whose only purpose is to move pixels is a defect. `review`
4. `update` names each transition worth animating as `Effect::Animate(Cue)`; the shell plays the cue and never diffs the model to guess what changed. `review`
5. Frames only while something moves: every `frame_due` source is a pure function of (layout, anchor, now) that never slides; spectrum frames only while it is on screen and playing or decaying; with nothing moving the loop blocks with no deadline. `review`
6. Every channel that carries traffic is bounded; the loop never blocks on a driver (`try_send`; full → the command is dropped and the port's congestion flag rises; a closed port or a driver that is not running drops it too, with no reason value); a driver raises its flag before it blocks on a full mailbox; one episode (`Episode { Clear, Reported }` per port) yields exactly one `DriverEvent::Full` and one toast, and ends at the first loop iteration where the port's flag stays down. `review`
7. A message with no visual change: no paint, no allocation. A burst of keys or messages is drained as one batch and paints once. Spectrum frames never queue and never wake the loop. Idle: every thread blocked, 0 % CPU. `review`
8. Playlist rows borrow precomputed display text, no `String` per row per frame, `Arc::clone` over deep clones; pixel components are memoized by (input, generation) and re-encode only on change; a cover change on a pixel terminal with Animations on fades in at most `COVER_CROSSFADE_STEPS` encoded steps, with Animations off it repaints once. `review`
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

- S4: `PlaybackRequest` carried inside driver data (`RemoteInput`, `MacosEvent::MediaKeyPressed`).
- S13: owner of volume (macOS vs app) and which effects stay relative.

Navidrome, closed by the streaming stage (decided 2026-10-07/08):

- G1 (closed): all HTTP is `RemoteJob`s of the `remote` driver; one job per variant runs at a time, and the driver queues connects, stars and play reports itself; token auth only; listings page by `Page`; redirects are followed by hand, at most 3, same host only.
- G2 (closed): server name, link and user live in `config.toml` `[[server]]` tables (`Account`); the password lives in the Keychain through keyring-core, read and written only by the remote driver; the Model keeps the `Session`.
- G3 (closed): `TrackSource::Server` names a server track; `remote` builds the stream URL from the `Session`, the kernel drives the download chunk by chunk (`MediaFetch`, `Download`), and audio plays `Media::Growing` up to the bound the kernel sends.
- G5 (closed): the browser shows one catalog at a time (`CatalogName`); each server's listings are a `Catalog`, and each listing answer carries its own `Revision`.

Open:

- G7: where widget constants live and where widget snapshots go.
