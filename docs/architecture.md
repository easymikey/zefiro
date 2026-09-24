# Architecture

This document describes how sifr is built as of the crate rebuild of September 2026. It is the input for the per-crate reviews (`docs/review-criteria.md`); the last section lists the places where an abstraction is suspected to leak.

## Pattern

sifr is The Elm Architecture (TEA) with drivers:

```
Message ──► kernel::update(&mut Model, Message) ──► Result<Cmd, Rejection>
   ▲                                                   │
   │                                            runtime::interpret
   │                                                   │
drivers (threads) ◄── commands ───────────────────────┤
   │                                                   ├── timers (Effect::After)
   └── Message ──► mailbox ──► event loop              └── ShellEffect ──► Shell
                                                                           │
                                          Shell::paint: Scene ─► FrameLayout ─► Pixels ─► Screen
```

- The kernel is the only place that decides. It is pure: no IO, no clock, no threads.
- The runtime performs what the kernel decided. Its interpreter is a lookup table.
- Drivers adapt external sources (audio device, file system, config files, macOS media keys, terminal input, OS signals) into `Message`s. A driver waits for its source; nothing polls.
- The shell (binary `sifr`) turns terminal input into messages and the model into a frame.
- Timers exist only in the kernel as `Effect::After { delay, message }`; they come back as `Message::Elapsed`.

## Crates and layers

Edges only point left. `runtime` never depends on `raster`, `widgets` or `terminal`; `terminal` never depends on `runtime`.

| layer | crate | role | depends on |
|---|---|---|---|
| 0 | `kernel` | `Model`, `Message`, `Cmd`/`Effect`, `update`, key routing, domain types | — |
| 1 | `config` | file formats: `config.toml`, `sifr-ui.toml` (appearance), themes, keymap; parse and format-preserving patch; settings rows | kernel |
| 1 | `library` | scan, tags, embedded covers, playlists; `execute(LibraryCmd)` | kernel |
| 1 | `audio` | playback engine on rodio, spectrum tap; `AudioLoop` | kernel |
| 1 | `macos` | media keys, Now Playing, system volume and output device (CoreAudio listeners); `SystemLoop` | kernel |
| 2 | `raster` | pure pixel images: progress bar, meters, vinyl, cover fitting | kernel, config |
| 2 | `runtime` | event loop, interpreter, timers, trace, driver threads (audio, library + covers + watch, config, macos), boot and drain | kernel, config, library, audio, macos |
| 3 | `widgets` | pure terminal view: `Scene`, `FrameLayout`, `Screen`, card, playlist, overlays, toast, animations, milkdrop, spectrum smoothing | kernel, config, raster |
| 4 | `terminal` | terminal IO: session, `InputLoop`, key conversion, capability probe, window colours, `Pixels` (image protocols) | kernel, config, raster, widgets |
| 5 | `sifr` | binary: command line, startup, signals, `Shell` implementation | all but raster, macos |

## Threads

| thread | waits on | sends |
|---|---|---|
| main (event loop) | `select` over input, mailbox, reloads, decoded covers; deadline = earliest of kernel timers, animation frame, immediate repaint, macOS run loop pump (N5 removes the pump) | commands to drivers; `ShellEffect`s and paints to the shell |
| audio | its command inbox (idle); `Engine::TICK` while busy (N4 removes it) | `Message::Audio(..)` |
| library | command inbox, `notify` events, cover requests, debounce deadline | scan results, covers, watch failures |
| config | command inbox, `notify` events on the config directory, save deadline | `Reload::Theme/Appearance`, kernel messages (keymap, custom rows, failures) |
| macos | command inbox, CoreAudio property listeners | `Message::SystemVolume`, output route changes, media keys |
| terminal input | crossterm `read` | `ShellInput::Terminal(Event)` |
| signals | signal-hook `Signals::forever()` | `ShellInput::Terminate` |

Every driver runs inside `spawn_driver`: a named thread under `catch_unwind`; on exit it reports `Message::Driver(driver, Stopped | Died(Panicked))`. `model.drivers` holds each driver's `DriverStatus`; the interpreter sends a command only to a `Running` driver (`gated`), otherwise it records `TraceEntry::Dropped`.

Hardware drivers are injected: `Runtime::boot(startup, paths, Hardware::system(&startup))` in the binary, `Hardware::new(stub_audio, spectrum, stub_system)` in tests, so boot and stop are tested without a sound device.

## Contracts

Kernel (`crates/kernel/src/update/mod.rs`):

```rust
pub fn update(model: &mut Model, message: Message) -> Result<Cmd, Rejection>;
```

Every sub-machine follows `Machine::transition(self, message) -> Result<(Self, Effect), Rejected<Self>>`. A message the current state does not accept is a `Rejection`, not a silent no-op.

Driver (`crates/runtime/src/driver.rs`):

```rust
pub trait DriverLoop<C>: Send + 'static {
    fn run(self, inbox: &Receiver<C>, mailbox: &Sender<Message>);
}
```

Shell (`crates/runtime/src/shell.rs`), implemented only by the binary:

| method | called when |
|---|---|
| `input(event, &Model) -> Option<Message>` | an input arrives (keys go through `kernel::route`; `Terminate` becomes the quit message) |
| `reloaded(Reload)` | a theme or appearance file changed on disk (not by our own write) |
| `effect(ShellEffect)` | the kernel asked for window colours, an animation cue, or an in-app appearance change |
| `cover(CoverDecoded)` | the library decoded the cover the shell asked for |
| `frame_due() -> FrameDue` | the loop computes its deadline: `At(instant)` only while an animation runs |
| `paint(View) -> Painted` | after a step whose `Change` was `Applied`, a reload or a cover |

## Flows

**Key press.** crossterm event → input thread → `ShellInput::Terminal` → `Shell::input` → `terminal::from_event` → `kernel::route(key, bindings)` → `Message` → `update` → `Cmd` → interpreter → driver commands / timers / shell effects → paint.

**Playback.** `update` returns `AudioCmd::Play` → audio inbox → engine opens the file → `AudioEvent`s back through the mailbox (`Started`, `Position`, `Ended`, `Rejected`) → `update`.

**In-app setting.** nudge key → `update` → `Effect::Setting(patch)` → interpreter sends the patch to the config driver (coalesced format-preserving save) and `ShellEffect::Appearance(patch)` to the shell in the same step → shell holds `appearance.patched(&patch)` → next frame. The config driver marks its own write as seen, so it never comes back as a reload.

**Hand edit of `sifr-ui.toml`.** `notify` event → config driver parses → `Reload::Appearance(file)` to the shell and `LoadedRequest::CustomRowsReloaded(config::custom_rows(&file))` to the kernel, so the settings rows show the file's values.

**Cover.** `Shell::paint` returns which cover it wants in `Painted` → runtime asks the library thread (`CoverRequest`) → decode + fit → `CoverDecoded` → `Shell::cover` maps it to `terminal::DecodedCover` → `Pixels` encodes it once per (path, rect) → placed after `Screen` in the same `terminal.draw`.

**Timer.** `Effect::After { delay, message }` → `Timers` slot (one per kind: toast, sleep; a new one replaces the old) → the loop's deadline is the earliest slot → on wake every due timer, sorted by deadline, becomes `Message::Elapsed` → `update`; a timer whose `Revision` is stale is rejected by the kernel and causes no repaint.

**One frame.** `Scene { model, theme, appearance, bindings, spectrum, pixel_path, cell_aspect, clock, now_unix, music_dir, sleep_left }` → `FrameLayout::new(&scene, area)` computes every rect once → `Pixels::refresh` returns the cover art → one `terminal.draw`: `Screen` paints text cells, then `Pixels::place` puts image protocols last, skipping rects covered by overlays or toasts.

**Stop.** `q`, a signal or a worker panic → kernel quit → `Flow::Stop` → `drain`: flush pending saves, stop drivers, join those that reported.

## Events (target design)

Decided on 2026-09-24 (`docs/superpowers/plans/2026-09-24-event-model-design.md`, §8 and §9); not implemented yet. Where the sections above describe today's code, this section is the target, and the tasks in §6 of that note get there.

sifr routes events like an operating system. The kernel receives only decisions and domain facts, so it is never flooded; motion belongs to the shell, the way a compositor owns vsync.

### Three event classes

| class | what | path | reaches the kernel |
|---|---|---|---|
| **fact** | a decision or a domain fact: key press, media key, track finished or changed, loaded, device or output route changed, system volume changed, failure, scan result, driver died or congested | driver → its typed `Outbox` → bounded mailbox → `update(model, message, now)` | yes, as `Message` |
| **driver internal** | how a driver does its work: buffer refills, decode and preload progress, retries, debounce, save coalescing, raw cpal, CoreAudio, AppKit and notify callbacks | stays in the driver thread; only the resulting fact leaves | no |
| **stream** | a value that only matters when painted: spectrum samples, levels, the decoded cover, the reloaded theme and appearance | latest-value cell, overwritten, read by the shell at paint | no |

A value is a stream when losing an intermediate value is harmless and only the latest matters. It is a fact when the kernel must decide on it (preload point, A-B end, sleep, Now Playing, toast). The kernel is never sent a value only because time passed.

### Linux mapping

| Linux | sifr |
|---|---|
| interrupt top half | an OS callback (cpal error, CoreAudio listener, `MPRemoteCommand`, rodio source `next`) sets a flag or does one `try_send` into a bounded(1) doorbell; it never allocates, locks, blocks or decides |
| bottom half / workqueue | the driver thread: drains the doorbell, reads the flags, runs its pure `step`, sends zero or more facts |
| syscall | a `Message` into `update` |
| epoll | the one runtime `select` over input, mailbox and doorbells, with one deadline |
| timerfd | kernel `Effect::After` → `Message::Elapsed`, only for decisions |
| shared memory | lock-free cells: `triple_buffer`, atomics, `arc-swap` |
| vsync / compositor | the shell's frame clock: frames only while something moves |
| input coalescing | a burst is drained in one batch and paints once |
| scheduler priority | input is stepped before driver facts in every batch |
| init / systemd | per-driver supervision strategy; the kernel decides, the runtime performs |
| built-in drivers, no modules | a static driver set declared in one registry table |

### Registry

The driver set is static: audio, library, config, macOS. `runtime::registry` holds one hand-written `const` row per driver and a hand-written typed `Ports` struct (no macro); both are exhaustive over `Driver`, so a new driver fails to compile until its row is filled. A row states the thread and placement (`Worker | MainThread`), the command inbox capacity, the fact type the driver may send, the cells it writes, its supervision strategy and its congestion flag. `Wiring`, `spawn_driver`, `drain` and `Port::send` (gate, overflow, trace) read the row; nothing else knows a driver.

### Cells

A cell holds one latest value; a writer overwrites, the shell reads at paint, nothing queues. Cells are lock-free, because the audio thread must never wait on a lock held by the shell:

- `triple_buffer` (single producer, single consumer, wait-free, no allocation) for spectrum samples and levels;
- `std::sync::atomic` for scalars (published gain, flags, congestion flags);
- `arc-swap` for the theme, the appearance and the decoded cover.

A cell whose change must be painted rings a bounded(1) doorbell once per new value. The displayed playhead is not a cell: the kernel holds the anchor `Playhead { offset, since, speed }`, and the shell computes `position_at(now)` at paint. The audio driver re-anchors only on actions (start, pause, seek, speed, track change, device reopen) and after a stall it detects.

### Time and frames

The kernel reads no clock: `update(model, message, now)`. Kernel timers (`Effect::After`) exist only for decisions: `Timer::Mark` (the preload-due point or the A-B end, armed by `next_decision`, none when no decision lies ahead), the sleep timer, toast expiry and a supervision backoff. A timer never exists for display. `Timers` is a keyed map over `Timer`.

The shell is the compositor. `frame_due(view)` is the earliest moment something on screen moves, each source a pure function of (layout, anchor, now) that never slides:

| source | next frame | while |
|---|---|---|
| animation stage, cover crossfade | last paint + 33 ms | the effect runs, plus one closing frame |
| spectrum, milkdrop, spinning vinyl | last paint + 33 ms | `Playing` and the widget is on screen; after a pause, until every band decays to zero |
| progress bar | the moment the bar visibly changes (a pixel of the pixel bar, a cell of the text bar) | `Playing` |
| clock digits | the next whole second of `position_at`, divided by the speed | `Playing` and a clock is shown |
| sleep countdown | the next whole minute (minutes only) | a sleep timer runs and is shown |

When nothing moves, `frame_due` is `Settled` and the loop blocks with no deadline.

### Loop and priorities

One `select` over input, the mailbox and the cell doorbells, with one deadline: the earliest of the kernel timers and the frame clock. After any wake the loop drains input first, then the mailbox; every ready item forms one batch, and a batch paints once. A key-triggered batch paints at once; only self-moving things follow the 33 ms grid. `frame_due` is computed once per iteration with the same `now`. Timers fire after the batch.

### Backpressure

Every channel is bounded: mailbox 256, each command inbox 64 per row, input 256, notify and decode workers 64, doorbells 1.

- The loop never blocks on a driver. Each driver drains every ready command and coalesces idempotent ones (volume, speed, seek) to the last; the runtime only `try_send`s, and `Full` records `TraceEntry::Dropped { reason: Full }` and raises the row's congestion flag.
- A driver that finds the mailbox full raises its row's congestion flag, then blocks, so only the culprit slows down.
- After every batch the runtime swaps each flag: a raised flag with no open episode steps one `DriverMessage::Congested`; a clear flag ends the episode. The kernel shows one toast per episode naming the driver, and a trace entry.
- No cycle can deadlock: the loop only `try_send`s to drivers, drivers only `send` to the loop.

### Supervision

`Died` is a fact; the kernel decides with a pure `supervise(strategy, history, now)`, and the runtime performs the decision from the registry row (respawn, resend the boot commands). Each row picks one strategy, so switching is a one-line change:

- `Restart { attempts, within, then }` (systemd `StartLimitBurst`/`IntervalSec`, Erlang intensity/period);
- `Backoff { attempts, first, longest, then }`, restarts delayed through `Effect::After`;
- `Degrade(Notice)`: keep running without the driver; `Notice` is `Toast` or `Silent`;
- `Fatal`: quit with an error.

`then: Fallback` is `Degrade(Notice)` or `Fatal`, applied once the attempts run out. Defaults: audio `Restart { 3, 60 s, then: Degrade(Toast) }`, library `Restart { 1, 60 s, then: Degrade(Toast) }`, config `Degrade(Toast)`, macOS `Degrade(Silent)`.

### Dependency inversion, FP style

1. The abstraction is a data type, not a trait. The kernel owns the contracts: facts in (`Message`, one fact enum per driver: `AudioEvent`, `LibraryFact`, `ConfigFact`, `SystemEvent`) and commands out (`Cmd`/`Effect`). `update` describes what should happen; the interpreter decides how (Elm `Cmd`, free monad). The kernel is tested with no mocks.
2. A driver is a state machine too: a pure `step(state, input) -> (state, facts)` tested as a table, and a thin IO loop (select, step, send) with no logic.
3. A capability is a value: a driver receives `Outbox<ItsFact>` and can send only its own facts; the shell receives only its cells. Media keys arrive as `SystemEvent::MediaKey(gesture)`; the kernel maps the gesture.
4. Traits exist only at the hardware edge: one narrow trait per kind of IO (`AudioBackend`, `SystemControls` …), methods take and return data, static dispatch through generics (`Hardware<A, M>`), never `dyn`. Fakes plug in over the same real channels.
5. The view is a function: `view(&Model, &Cells) -> Frame` is pure; `paint(frame)` is the shell's only side effect.
6. One composition root: `main` → `Runtime::boot` → the registry picks the concrete types. Below it nothing knows it runs on real CoreAudio.

### Stop

The loop stops reading input; the kernel's quit `Cmd` has already stopped audio and cleared Now Playing. `drain` walks the registry in order — audio (silence first), macOS, library, config (flushes pending saves) — waits for `Stopped | Died` up to two seconds in total, joins those that reported, and on macOS ends the main run loop.

## Suspected leaks (for the reviews)

Each row is a question, not a verdict. Rows marked *Resolved* or *Narrowed* are answered by the Events section once its tasks land.

1. **Drivers speak kernel.** `audio`, `library`, `macos` build `kernel::Message` directly and `library::execute` matches `LibraryCmd`. Adapters are therefore not reusable outside sifr and the kernel's message shape leaks into four crates. Alternative: each driver has its own event type and runtime maps it. *Narrowed by Events (FP rule 3): each driver sends only its own fact enum through a typed `Outbox`; the enums stay in the kernel by decision (Q13).*
2. **`audio` re-exports `kernel::AudioEvent`** and **`macos` re-exports `objc2::MainThreadMarker`** and `pump_main_run_loop`: a foreign type in a public surface. *Half resolved by Events: N5 M1 deletes `pump_main_run_loop` and the `MainThreadMarker` re-export; the `AudioEvent` re-export stays open.*
3. **Kernel surface is doubled.** `pub mod cmd, domain, message, search, update` plus `pub use` of the same items: two paths to every type, and the whole `domain` is public, including internals of sub-machines.
4. **Config surface is doubled** the same way (`pub mod` + `pub use`), and exposes two free `patched`/`appearance_patched` functions next to `AppearanceFile::patched`.
5. **`raster` depends on `config`**: pure pixel code knows file formats (theme colours). Possibly only a colour type is needed.
6. **`widgets::Scene` borrows the whole `&Model`.** It is the root view, so Demeter allows it, but every widget read-model method sits on `Scene`; check that no component below the root reaches into `Model`.
7. **Name clash `widgets::Pixels` (geometry unit) vs `terminal::Pixels` (image layer).** Also `terminal::CoverArtOwner` exists only to own what `widgets::CoverArt` borrows.
8. **`terminal::UnknownThemeError`** in window colours: the terminal layer knows theme names.
9. **Binary reaches past runtime**: `sifr` depends on `audio` and `library` directly (engine config, playlist load at startup). Check whether runtime should own these.
10. **`CoverRequest.side: u32`** is a bare pixel count in runtime's public API; the pixel unit type lives in widgets, which runtime cannot see.
11. **Main-thread pump** (`Runtime::pump`, `PUMP_CAP` 100 ms) and **audio `Engine::TICK`** are the last periodic wake-ups (tasks N5, N4). *Resolved by Events: N4 and N5 remove both; the frame clock moves the display.*
12. **macOS volume read spawns a child process** although CoreAudio listeners are now in place; `rebind()` re-subscribes on every volume event. *Resolved by Events (task M2): CoreAudio volume properties, a bounded(1) doorbell, `rebind` only on a default-device change.*
13. **Timers are fixed slots** (toast, sleep). A third kind needs a new field; a keyed map would not. *Resolved by Events: `Timer::Mark` and the supervision backoff make four kinds, so `Timers` becomes a keyed map (task R2).*
14. **Settings rows**: `CustomSetting.position` means the current value's index, not the row's index; the name invites the wrong reading.
15. **Trace**: `TraceEntry` mixes dropped commands, rejected settings and platform facts (`ControlsUnattached`); check it is one concept.
