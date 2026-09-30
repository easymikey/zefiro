# Architecture

This document describes how sifr is built as of the crate rebuild of September 2026. It is the input for the per-crate reviews (`docs/review-criteria.md`); the last section lists the places where an abstraction is suspected to leak.

## Pattern

sifr is The Elm Architecture (TEA) with drivers:

```
Message ──► kernel::update(&mut Model, Message, Moment) ──► Result<Cmd, Rejection>
   ▲                                                            │
   │                                                     runtime::interpret
   │                                                            │
drivers (threads) ◄── commands ─────────────────────────────────┤
   │                                                            ├── timers (Effect::After)
   └── fact ──► DriverSender ──► event loop                          └── ShellEffect ──► Shell
                                                                                    │
                                            Shell::paint: view ─► Scene ─► FrameLayout ─► Screen
```

- The kernel is the only place that decides. It is pure: no IO, no clock, no threads.
- The runtime performs what the kernel decided. Its interpreter is a lookup table.
- Drivers adapt external sources (audio device, file system, config files, macOS media keys, terminal input, OS signals) into events. A driver waits for its source; nothing polls.
- The shell (binary `sifr`) turns terminal input into messages and the model into a frame.
- Timers exist only in the kernel as `Effect::After { delay, message }`; they come back as `Message::Elapsed`.

## Crates and layers

Edges only point left. `runtime` never depends on `raster`, `widgets` or `terminal`; `terminal` never depends on `runtime`.

| layer | crate | role | depends on |
|---|---|---|---|
| 0 | `kernel` | `Model`, `Message`, `Cmd`/`Effect`, `update`, key routing, `Machine`, `Outbox`, supervision, domain types | — |
| 1 | `config` | file formats: `config.toml`, `sifr-ui.toml` (appearance), themes, keymap; parse and format-preserving patch; settings rows | kernel |
| 1 | `library` | scan, tags, embedded covers, playlists; `execute(LibraryCmd)` | kernel |
| 1 | `audio` | playback engine on rodio, spectrum tap; `AudioLoop` | kernel |
| 1 | `macos` | media keys, Now Playing, system volume and output device (CoreAudio listeners); `MacosLoop`, `MainLoop` | kernel |
| 2 | `raster` | pure pixel images: meters, vinyl, cover fitting, progress geometry and colours | kernel, config |
| 2 | `runtime` | event loop, interpreter, timers, trace, cells, registry, driver threads (audio, library + covers + watch, config, macos), start, drain, `host` | kernel, config, library, audio, macos |
| 3 | `widgets` | pure terminal view: `Scene`, `FrameLayout`, `Screen`, card, playlist, overlays, toast, animations, milkdrop, spectrum smoothing | kernel, config, raster |
| 4 | `terminal` | terminal IO: session, `InputLoop`, key conversion, capability probe, window colours, `Pixels` (image protocols) | kernel, config, raster, widgets |
| 5 | `sifr` | binary: command line, startup, signals, the `Shell` implementation (`view`, `Motion`, `Painter`, frame clock) | all but macos |

## Threads

| thread | waits on | sends |
|---|---|---|
| main (macOS only) | the AppKit run loop (`macos::MainLoop`), which `runtime::host` runs while the event loop lives on a thread of its own | media key events through the mailbox |
| event loop (`sifr-event-loop`) | one `select` over input, mailbox and the cell doorbell; deadline = earliest of kernel timers, frame clock, pending repaint | commands to drivers; `ShellEffect`s and paints to the shell |
| audio | its command inbox | `AudioEvent` events |
| library | command inbox, `notify` events, decoded covers, debounce deadline | scan results, `LibraryEvent`s, watch failures; decoded covers go to the cover cell |
| cover (`sifr-cover`) | a doorbell for its one pending request | decoded covers |
| config | command inbox, `notify` events on the config directory, save deadline | theme and appearance cells, `ConfigEvent`s (keymap, custom rows, failures) |
| macos | command inbox, CoreAudio property listeners | `MacosEvent::Volume`, output route changes, media keys, watch failures |
| terminal input | crossterm `read` | `ShellInput::Terminal(Event)` |
| signals | signal-hook `Signals::forever()` | `ShellInput::Terminate` |

Off macOS `host` runs the body inline. Every driver runs inside `spawn_driver`: a named thread under `catch_unwind`; on exit it reports `Message::Driver(driver, Stopped | Died(..))`. `model.drivers` holds each driver's `DriverStatus`; a `Port` sends a command only to a `Running` driver, otherwise the interpreter records `TraceEntry::Dropped`.

Hardware drivers are injected: the binary passes the real `Spawners` to `Runtime::start(startup, &paths, &spawners)`, tests pass stubs (recording or panicking audio), so start and stop are tested without a sound device.

## Contracts

Kernel (`crates/kernel/src/update/mod.rs`):

```rust
pub fn update(model: &mut Model, message: Message, now: Moment) -> Result<Cmd, Rejection>;
```

Every sub-machine implements `Machine::transition(self, message) -> Result<(Self, Effect), Rejected<Self>>` (`crates/kernel/src/update/machine.rs`); the provided `update(&mut self, message) -> Result<Effect, Rejection>` writes the state back on both branches. A message the current state does not accept is a `Rejection`, not a silent no-op. Driver-side machines (`ConfigState`, `LibraryState`, the audio engine, the macOS volume echo) use the same trait.

Driver (`crates/runtime/src/driver.rs`, crate-private):

```rust
pub(crate) trait DriverLoop<C, F>: Send + 'static {
    fn run(self, inbox: &Receiver<C>, outbox: &DriverSender<F>);
}
```

Shell (`crates/runtime/src/shell.rs`), implemented only by the binary:

| method | called when |
|---|---|
| `input(Self::Input) -> Reaction` | an input arrives: `Reaction::Message(Message)`, `Repaint` or `Ignored` |
| `effect(ShellEffect)` | the kernel asked for window colours or an animation cue |
| `frame_due(&FrameInput) -> FrameDue` | the loop computes its deadline: `At(moment)` only while something moves, else `Settled` |
| `paint(FrameInput) -> Painted` | the loop decided to paint; `Painted` carries the cover the frame wants, the visible row count and paint failures as messages |

`FrameInput` gives the shell the model, the spectrum tap, the cells, the sleep deadline and `now`.

## Flows

**Key press.** crossterm event → input thread → `ShellInput::Terminal` → `Shell::input` → `Reaction::Message(Message::Key(KeyPress))` → `Runtime::step` → `kernel::update` routes the key through `kernel::route` and the bindings → `Cmd` → interpreter → driver commands / timers / shell effects → paint.

**Playback.** `update` returns `AudioCmd::Load` → audio inbox → engine opens the file → `AudioEvent`s back through `DriverSender<AudioEvent>` → `update`. A command the engine refuses comes back as `DriverMessage::Rejected { input }`.

**Volume.** `update` returns `MacosCmd::Volume` → macos inbox → CoreAudio write; the volume echo swallows the listener event of our own write. A change made outside sifr arrives as `MacosEvent::Volume`. If the hardware watch cannot start, `MacosEvent::HardwareWatchFailed` becomes a toast.

**In-app setting.** nudge key → `update` → `Effect::Setting { id, option }` → `ConfigCommand::Setting` → the config machine patches its appearance, queues the save in `SaveQueue` (coalesced, format-preserving) and outputs `Published::Appearance` → appearance cell and doorbell → the next paint installs it. The machine marks its own write as seen, so it never comes back as a reload.

**Hand edit of `sifr-ui.toml`.** `notify` event → config machine polls the files → `Published::Appearance(file)` into the cell and `ConfigEvent::CustomRowsReloaded(rows)` to the kernel, so the settings rows show the file's values. A theme file works the same way with `Published::Theme` and `ConfigEvent::ThemeReloaded`.

**Cover.** `Shell::paint` returns the cover it wants in `Painted` → `Runtime::request_cover` (once per distinct request) → library port → library machine: a cached decode is published at once, otherwise the cover worker decodes and fits it → `LatestSender<CoverDecoded>` into the cover cell and the doorbell → the next paint takes it from the cells → `Pixels` encodes it once per (path, rect) → placed after `Screen` in the same `terminal.draw`. The cover cache is state of the library machine.

**Timer.** `Effect::After { delay, message }` → `Timers` slot (one per `Timer` kind: toast, sleep, mark, and a restart slot per driver; a new one replaces the old) → the loop's deadline is the earliest slot → on wake every due timer, sorted by deadline, becomes `Message::Elapsed` → `update`; a timer whose `Revision` is stale is rejected by the kernel and causes no repaint.

**One frame.** `view(&FrameInput, &Presentation, &Motion)` builds `Scene { model, theme, color_depth, appearance, bindings, spectrum, pixel_path, cell_aspect, clock, now, music_dir, sleep_left }` → `FrameLayout::new` computes every rect once → `Pixels::refresh` returns the cover art → `Painter::paint`: one `terminal.draw`, `Screen` paints text cells, then `Pixels::place` puts image protocols last, skipping rects covered by overlays or toasts.

**Stop.** `q`, a signal or a worker panic → kernel quit → `Flow::Stop` → `drain`: drop the ports, stop drivers (the config machine flushes pending saves on `Stopping`), join those that reported.

## Events

sifr routes events like an operating system. The kernel receives only decisions and domain events, so it is never flooded; motion belongs to the shell, the way a compositor owns vsync. The design note is `docs/superpowers/plans/2026-09-24-event-model-design.md`.

### Three event classes

| class | what | path | reaches the kernel |
|---|---|---|---|
| **fact** | a decision or a domain fact: key press, media key, track finished or changed, loaded, device or output route changed, system volume changed, failure, scan result, driver died or congested | driver → `DriverSender<F>` (`Outbox<F>`) → bounded mailbox → `update(model, message, now)` | yes, as `Message` |
| **driver internal** | how a driver does its work: buffer refills, decode and preload progress, retries, debounce, save coalescing, raw cpal, CoreAudio, AppKit and notify callbacks | stays in the driver thread; only the resulting fact leaves | no |
| **stream** | a value that only matters when painted: spectrum samples, levels, the decoded cover, the reloaded theme and appearance | latest-value cell, overwritten, read by the shell at paint | no |

A value is a stream when losing an intermediate value is harmless and only the latest matters. It is a event when the kernel must decide on it (preload point, A-B end, sleep, Now Playing, toast). The kernel is never sent a value only because time passed.

### Linux mapping

| Linux | sifr |
|---|---|
| interrupt top half | an OS callback (cpal error, CoreAudio listener, `MPRemoteCommand`, rodio source `next`) sets a flag or does one `try_send` into a bounded(1) doorbell; it never allocates, locks, blocks or decides |
| bottom half / workqueue | the driver thread: drains the doorbell, reads the flags, runs its machine's `transition`, sends zero or more events |
| syscall | a `Message` into `update` |
| epoll | the one runtime `select` over input, mailbox and the cell doorbell, with one deadline |
| timerfd | kernel `Effect::After` → `Message::Elapsed`, only for decisions |
| shared memory | lock-free cells: `triple_buffer`, atomics, `arc-swap` |
| vsync / compositor | the shell's frame clock: frames only while something moves |
| input coalescing | a burst is drained in one batch and paints once |
| scheduler priority | input is stepped before driver events in every batch |
| init / systemd | per-driver supervision strategy; the kernel decides, the runtime performs |
| built-in drivers, no modules | a static driver set declared in one registry table |

### Registry

The driver set is static: audio, macOS, library, config. `runtime::registry` holds `REGISTRY`, one hand-written `const DriverRow` per driver, and `row(driver)`, which matches exhaustively over `Driver`, so a new driver fails to compile until its row is filled. A `DriverRow` states the thread name, the placement (`Worker | WorkerWithMainLoop`), the platform (`Every | Macos`), the command inbox capacity and the supervision strategy. The typed `Ports` struct in `runtime::port` (one `Port<C>` per driver, each with its congestion flag) is hand-written too, no macro. `Wiring`, `spawn_driver`, `drain` and `Port::send` (gate, overflow, trace) read the row; nothing else knows a driver.

### Cells

A cell holds one latest value; a writer overwrites, the shell reads at paint, nothing queues. Cells are lock-free, because the audio thread must never wait on a lock held by the shell:

- `triple_buffer` (single producer, single consumer, wait-free, no allocation) for spectrum samples (`SpectrumTap`);
- `std::sync::atomic` for scalars (congestion flags, the library overflow flag);
- `arc-swap` for the theme, the appearance and the decoded cover: `runtime::cells()` returns `Senders` (one `LatestSender<T>` per value, `publish` overwrites and rings), `Receivers` (one `LatestReceiver<T>` per value, `take` swaps the value out) and one bounded(1) doorbell receiver that the loop selects on. `Senders` and `cells()` are public so tests and other shells can build their own.

The displayed playhead is not a cell: the kernel holds the anchor `Playhead { offset, since, speed }`, and the shell computes `position_at(now)` at paint. The audio engine reports a new anchor (`AudioEvent::Playhead`) only after an action: start, play, pause, seek, speed, promote (track change). There is no stall detection; a stalled output keeps the last anchor until the next action.

### Time and frames

The kernel takes the time as an argument: `update(model, message, now)`. Kernel timers (`Effect::After`) exist only for decisions: `Timer::Mark` (the preload-due point or the A-B end, none when no decision lies ahead), `Timer::Sleep`, `Timer::Toast` and `Timer::Restart(driver)` for a supervision backoff. A timer never exists for display.

The shell is the compositor. `Shell::frame_due(view)` is the earliest moment something on screen moves; the sources live in `sifr/src/shell/frame_clock.rs` and read `Motion::on_screen`, each a pure function of the layout, the anchor and `now` that never slides:

| source | next frame | while |
|---|---|---|
| animation stage, cover crossfade | last paint + 33 ms | the effect runs, plus one closing frame |
| `spectrum_frame_due`: spectrum row, milkdrop | last paint + 33 ms | `Playing` and the widget is on screen; after a pause, until every band decays to zero |
| `progress_frame_due` | the moment the bar visibly changes: half a cell of the text bar (two steps per cell) | `Playing` |
| `clock_frame_due` | the next whole second of `position_at`, divided by the speed | `Playing` and a clock is shown |
| `sleep_frame_due` | the next whole minute (minutes only) | a sleep timer runs and is shown |

The vinyl does not spin. When nothing moves, `frame_due` is `Settled` and the loop blocks with no deadline.

### Loop and priorities

One `select` over input, the mailbox and the cell doorbell, with one deadline: the earliest of the kernel timers, the frame clock and the pending repaint. After any wake the loop gathers input first, then the mailbox, then the doorbell; every ready item forms one batch, and a batch paints once. Timers fire after the batch, then congestion is settled and shell effects are handed over.

The pending repaint is `Repaint { Settled, Now, Frame }` (`runtime/src/repaint.rs`). An input or a `Reaction::Repaint` raises it to `Now`, a fact or a doorbell to `Frame`, which waits for the 33 ms grid (`FRAME_INTERVAL`) since the last paint; `Settled` paints only when the shell's own `frame_due` has passed. A key-triggered batch therefore paints at once and only self-moving things follow the grid. `frame_due` is computed once per iteration with the same `now`.

### Backpressure

Every channel that carries traffic is bounded: mailbox 256, each command inbox 64 per row, input 256, library `notify` events 64, the cover worker one pending request behind a doorbell of 1, cell doorbell 1. The config `notify` callback feeds an unbounded channel.

- The loop never blocks on a driver: a `Port` only `try_send`s, and `Full` records `TraceEntry::Dropped { reason: Full }` and raises the port's congestion flag. A full library `notify` channel raises an overflow flag that becomes one rescan.
- A driver that finds the mailbox full raises its congestion flag, then blocks in `send` (`Delivery::Congested`), so only the culprit slows down.
- After every batch the runtime swaps each flag: a raised flag with no open episode steps one `DriverMessage::Congested`; a clear flag with a drained mailbox ends the episode. The kernel shows one toast per episode naming the driver.
- No cycle can deadlock: the loop only `try_send`s to drivers, drivers only `send` to the loop.

### Supervision

`Died` is a fact; the kernel decides with a pure `supervise(strategy, history, now) -> Decision` (`Restart`, `RestartAfter`, `Degrade`, `Quit`), and the runtime performs the decision (`Effect::Restart`, restart and resend the start commands). Each registry row picks one `Supervision`, so switching is a one-line change:

- `Restart { attempts, within, then }` (systemd `StartLimitBurst`/`IntervalSec`, Erlang intensity/period);
- `Backoff { attempts, first, longest, then }`, restarts delayed through `Effect::After`;
- `Degrade(Announce)`: keep running without the driver; `Announce` is `Toast` or `Silent`;
- `Fatal`: quit with an error.

`then: Fallback` is `Degrade(Announce)` or `Fatal`, applied once the attempts run out. Defaults (`Supervision::standard`): audio `Restart { 3, 60 s, then: Degrade(Toast) }`, library `Restart { 1, 60 s, then: Degrade(Toast) }`, config `Degrade(Toast)`, macOS `Degrade(Silent)`.

### Dependency inversion, FP style

1. The abstraction is a data type, not a trait. The kernel owns the contracts: facts in (`Message`, one fact enum per driver: `AudioEvent`, `LibraryEvent`, `ConfigEvent`, `MacosEvent`) and commands out (`Cmd`/`Effect`). `update` describes what should happen; the interpreter decides how (Elm `Cmd`, free monad). The kernel is tested with no mocks.
2. A driver is a state machine too: a pure `transition(self, input) -> Result<(Self, Effect), Rejected<Self>>` through the kernel `Machine` trait, tested as a table, and a thin IO loop (select, transition, perform the outputs) with no logic.
3. A capability is a value: a driver receives its `DriverSender<ItsFact>` and can send only its own facts. `kernel::Outbox<F>` is the one narrow trait behind it (`send(fact) -> Delivery`); `DriverSender<F>` implements it, and `AudioLoop::run` is generic over it. The shell receives only its cells. Media keys arrive as `MacosEvent::MediaKey(gesture)`; the kernel maps the gesture.
4. Traits exist only at the hardware edge and the outbox: one narrow trait per kind of IO, methods take and return data, static dispatch through generics, never `dyn`. Fakes plug in over the same real channels.
5. The view is a function: `view(&FrameInput, &Presentation, &Motion) -> Frame` is pure; `Motion::advanced` moves the animation state between frames and `Painter::paint` is the shell's only side effect.
6. One composition root: `main` → `Runtime::start` → the registry picks the concrete types. Below it nothing knows it runs on real CoreAudio.

### Stop

The loop stops reading input; the kernel's quit `Cmd` has already stopped audio and reset the window colours. `drain` drops the ports, waits for `Stopped | Died` from the drivers up to two seconds in total, joins those that reported (the config driver flushes pending saves when its inbox closes), and on macOS `host` then stops the main run loop.

## Suspected leaks (for the reviews)

Each row is a question, not a verdict.

1. **`macos` builds `kernel::Message`.** `Controls::attach` and `MainLoop::attach` take a `Sender<Message>` and wrap `MacosEvent::MediaKey` themselves, and `library::execute` matches the kernel's `LibraryCmd`. `audio`, `library` and `config` already send only their own fact enum through an `Outbox`; the enums stay in the kernel by decision (Q13). Should `macos` take an `Outbox<MacosEvent>` too?
2. **Kernel surface is doubled.** `pub mod cmd, domain, message, outbox, search, update` plus `pub use` of the same items: two paths to every type, and the whole `domain` is public, including internals of sub-machines.
3. **Config exports free `patched` and `patch_appearance_text`** next to `AppearanceFile::patched`.
4. **`raster` depends on `config`**: pure pixel code knows file formats (theme colours). Possibly only a colour type is needed.
5. **Name clash `widgets::Pixels` (geometry unit) vs `terminal::Pixels` (image layer).** Also `terminal::CoverArtOwner` exists only to own what `widgets::CoverArt` borrows.
6. **`terminal::UnknownThemeError`** in window colours: the terminal layer knows theme names.
7. **Binary reaches past runtime**: `sifr` depends on `audio` and `library` directly (engine config, playlist load at startup). Check whether runtime should own these.
8. **`CoverRequest.side: u32`** is a bare pixel count in runtime's public API; the pixel unit type lives in widgets, which runtime cannot see.
9. **Trace**: `TraceEntry` mixes rejected messages, dropped commands, join and restart failures and platform events (`ControlsUnattached`); check it is one concept.
