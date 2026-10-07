# Architecture

How sifr is put together: crates and layers, threads, the message path, the driver registry and the cells. Every rule, trait signature and name lives in `docs/conventions.md` (cited as §n); why the design is this way is `docs/principles.md`. Where this file and the rulebook disagree, the rulebook wins.

## Pattern

sifr is The Elm Architecture (TEA) with drivers (conventions §1):

```
Message ──► kernel update(&mut Model, Message, Moment) ──► effects
   ▲                                                        │
   │                                               runtime interpreter
   │                                                        │
   │          ┌── Effect::X(XCmd) ──► port ──► DriverLoop ──► XDriver (transition / execute)
   │          ├── Effect::After { delay, timer } ──► timers
   │          └── shell effects (window colours, animation cues) ──► Shell
   │                                                        │
   └──────── XEvent ──► DriverLoop ──► inbox ◄────────────────┘
                                          Shell::paint: Scene ─► FrameLayout ─► the `screen` module
```

- The kernel is the only place that decides, and it is pure. `update` drains follow-up messages itself and returns the collected effects, or `Unhandled` when the message has no transition in the current state (§3.2, §3.4).
- The runtime performs what the kernel decided; its interpreter is a lookup with no decisions.
- Drivers adapt external sources (audio device, library files, config files, macOS media keys and system volume) into events. A driver waits for its source; nothing polls.
- The shell (binary `sifr`) turns terminal input into messages and the model into a frame.
- Timers exist only in the kernel as `Effect::After { delay, timer }`; they come back as `Message::Elapsed(Timer)`.

## Crates and layers

The layer map is conventions §1.1; edges only point down the table.

| layer | crate | role |
|---|---|---|
| 0 | `kernel` | `Model`, `Message`, `Cmd`/`Effect`, `update`, key routing, the `Machine` and `Driver` traits, supervision, domain types |
| 1 | `config` | file formats: `config.toml`, `sifr-ui.toml` (appearance), themes, keymap; parse and format-preserving patch; settings rows; `ConfigDriver` |
| 1 | `library` | scan, tags, embedded covers, playlists, history, favorites; `LibraryDriver` |
| 1 | `audio` | playback engine: symphonia decode, the feeder job, the mixer on the cpal callback with varispeed over rubato, spectrum tap; `AudioDriver` |
| 1 | `macos` | media keys, Now Playing, system volume and output device (CoreAudio listeners); `MacosDriver`, `MainLoop` |
| 2 | `runtime` | event loop, interpreter, timers, cells, registry, ports, `DriverLoop`, start, drain, `host` |
| 2 | `widgets` | pure terminal view: `Scene`, `FrameLayout`, the `screen` module, card, playlist, overlays, toast, animations, milkdrop, spectrum smoothing, pixel images |
| 3 | `terminal` | terminal IO: session, input, key conversion, capability probe, window colours, image protocols |
| 4 | `sifr` | binary: command line, startup, signals, the `Shell` implementation (view, presentation, motion clock, painter) |

`macos` is a target-gated dependency (§4.9): off macOS it is not linked.

## Threads

| thread | waits on | sends |
|---|---|---|
| main (macOS only) | the AppKit run loop (`MainLoop`), which `runtime::host` runs while the event loop lives on a thread of its own | remote commands into the macOS driver's inbox |
| event loop | one `select` over input, mailbox and the cell doorbell; deadline = earliest of kernel timers, frame clock, pending repaint | commands to drivers; shell effects and paints to the shell |
| one per driver (audio, library, config, macOS) | its `DriverLoop`: the command inbox and the driver's own sources (job results, stream items, callback input) | `XEvent`s into `inbox`; stream values into cells |
| job workers | one worker per job kind, one job at a time (track decode, cover decode, device list); the audio feeder is a job that runs as long as the audio driver | the job result back to the driver's inbox; the feeder also wakes the audio driver |
| cpal render callback | the output device's buffer clock | nothing: `Mixer::mix` only reads and writes rtrb rings, `triple_buffer` cells and atomics (see Audio path) |
| callbacks (CoreAudio, cpal stream errors, `notify`) | threads the OS or a library owns (§4.5b) | one message or flag into the driver's inbox |
| terminal input | crossterm `read` | terminal events to the event loop |
| signals | signal-hook | terminate |

Off macOS `host` runs the body inline. Every driver thread runs under `catch_unwind`; on exit it reports `Message::Driver { driver_name, event: Stopped | Died(..) }`. `model.drivers` holds each driver's status; a port sends a command only to a running driver with an open port, otherwise it drops the command; no reason value is kept.

Hardware drivers are injected: the binary passes the real spawners to `Runtime::start`, tests pass stubs, so start and stop are tested without a sound device.

## Message path

**Key press.** crossterm event → input thread → event loop → `Shell::input` → `Message::Key(KeyPress)` → `update` routes it through the key context stack (§3.8) → effects → interpreter → driver commands, timers, shell effects → paint.

**Playback.** `update` returns `Effect::Audio(AudioCmd::Load(..))` → audio port → `DriverLoop` delivers `AudioMessage::Cmds(Cmds { cmds, at })` → `AudioDriver::transition` → `execute` opens the file → `AudioEvent`s → `DriverLoop` → `inbox` → `update`.

**Volume.** `update` returns `MacosCmd::SetVolume` → macOS driver → CoreAudio write; the driver machine swallows the listener echo of its own write. A change made outside sifr arrives as `MacosEvent::VolumeChanged`.

**In-app setting.** a `Step` key → `update` → `Effect::Config(ConfigCmd::SetAppearance { .. })` → the config driver patches its appearance, schedules the save (coalesced, format-preserving) and publishes the appearance into its cell → the next paint installs it. The driver marks its own write as seen, so it never comes back as a reload.

**Hand edit of `sifr-ui.toml`.** a `notify` stream item → the config driver re-reads the file → the appearance into the cell and `ConfigEvent::AppearanceReloaded` to the kernel, which derives the settings rows from it, so they show the file's values. A theme file works the same way with `ConfigEvent::ThemeReloaded`; with Animations on, its cue fades the whole screen at once in a quick fade over `TIMINGS.screen_wash` (150 ms), every cell from the colours of the frame on screen (`PaintedCell`, kept by the painter only while Animations are on) to the new theme, and the window colours follow the same fade. A vinyl the theme only restyles (same track, same cover) repaints once. A layout change has no transition: the new layout shows at once.

**Cover.** the kernel asks for a cover with `Effect::Library(LibraryCmd::DecodeCover)` when track, side or mode changes → the library driver answers once per distinct request → a cached decode is published at once, otherwise a `CoverJob` runs on a worker → the decoded cover goes into the cover cell and rings the doorbell → the next paint takes it → the terminal encodes it once per (path, rect) and places it after the text in the same draw. A cover change (another cover path) in the same rect on a pixel terminal with Animations on fades in at most `COVER_CROSSFADE_STEPS` encoded steps over `TIMINGS.cover_crossfade`; with Animations off it repaints once.

**Timer.** `Effect::After { delay, timer }` → one slot per `Timer` kind (`Toast`, `Sleep`, `Lookahead`; a new one replaces the old) → the loop's deadline is the earliest slot → each due timer becomes `Message::Elapsed(timer)` → `update`; a timer whose `Revision` is stale returns `Err(Unhandled)` and causes no repaint.

**One frame.** the shell builds `Scene::from_model(&Model, ScenePresentation)` → `FrameLayout` computes every rect once → one terminal draw: the `screen` module paints text cells, then image protocols are placed last, skipping rects covered by overlays or toasts.

**Stop.** `q`, a signal or a fatal driver decision → the kernel sends the stop commands (audio stop, window colours reset, `ConfigCmd::Flush`) and `Quit` → drain: drop the ports, wait for `Stopped | Died` from each driver up to two seconds in total, join those that reported; on macOS `host` then stops the main run loop.

## Audio path

The `audio` crate decodes with symphonia and plays through one cpal output stream; the callback side is built from rtrb rings, `triple_buffer` cells, atomics and a rubato resampler, nothing else.

- **Callback.** The cpal render callback is `Mixer::mix`. Each call takes at most `MIXER_ORDERS` `MixerOrder`s (attach, queue, promote, retire, drop, transport, role playback, speed, seek) from a wait-free rtrb ring that the driver's `MixerControl` fills, mixes the current, incoming and outgoing `Voice` block by block, pushes the mixed block into the spectrum window and converts it to the device sample type. A voice reads its `FeedSource`: decoded `Chunk`s arrive through one rtrb ring and go back empty through another, so no sample buffer is allocated or freed on the callback. A voice the mixer replaces or that ends goes into the retired ring (`RetiredVoices`, held by the driver's `Output`), so it is never freed on the callback. Gain, ramp and fade start reach each voice's `Envelope` through a `triple_buffer` cell of `Order`, written by its `EnvelopeControl`; position, playback and the finished flags come back through atomics (`EnvelopeReadout`). The callback sends no signal: it touches no channel, so it never blocks.
- **Feeder.** On `AudioMessage::Started` the driver starts the feeder as an ordinary `DriverLoop` job, `AudioJob::Feed`, on a worker of its own; `serve` runs until the driver drops its feed sender, then answers `AudioMessage::Fed`. Each new voice's `Feed` is primed with `PRIME_CHUNKS` chunks on the driver thread and handed over as `FeedCmd::Serve`. The driver paces the feeder with `FeedCmd::Pace(Playback)`: while playing it refills every `FEED_PERIOD` (20 ms), while paused it blocks on its inbox. The feeder reads each envelope's flags and wakes the driver with one `DeckEvent::Woke(Revision)` (`Wake { Sent, Pending }` keeps a wake that a full channel refused); the driver then takes the signals as `AudioMessage::SignalsTaken`. A decode error reaches the engine as `EngineMessage::Interrupted`.
- **Seek in two phases.** `Deck::seek` orders `MixerOrder::Seek(target)`. Phase one, on the callback: `FeedSource::seek` raises the seek generation and publishes the target and the new position through the `SeekTarget` atomics; while the voice plays, chunks of the older generation keep playing until the first chunk of the new one arrives, and a paused voice drops them. Phase two, on the feeder: it sees the new generation, seeks the decoder and fills from where it landed, using `SEEK_SPARES` spare chunks so the new audio does not wait for the ring to drain. A refused seek reports `EngineMessage::Error` and stores the kept generation, and the source plays on from it.
- **Varispeed.** Each voice converts the file's rate to the device rate times the speed with a rubato sinc resampler (`Varispeed`, in blocks of `VARISPEED_FRAMES`). At speed one on a device of the file's rate it copies frames bit-exact (`Conversion::Native`) and primes the resampler at the first speed change. `MixerOrder::Speed` sets the speed of every voice.
- **Declick.** A transport change ramps each voice linearly over `DECLICK_FRAMES` (256) frames: play fades in, pause plays the ramp down to silence before the voice stops reading; a queued gapless voice takes over the playback and ramp of the voice it follows.
- **RTSan.** `Mixer::mix` and `FeedSource::read` carry `#[sanitize(realtime = "nonblocking")]`; `scripts/rtsan.sh` runs the audio tests with `-Zsanitizer=realtime`, so an allocation, lock or blocking call reached from the callback fails the run.

## Event classes

Three classes, one path each (§12.1):

| class | what | path | reaches the kernel |
|---|---|---|---|
| fact | key press, media key, track finished or changed, loaded, device or route changed, system volume changed, failure, scan result, driver died or full | driver → `DriverLoop` → `inbox` → `update` | yes, as `Message` |
| driver internal | buffer refills, decode progress, retries, debounce, save coalescing, raw callbacks | stays on the driver thread; only the resulting fact leaves | no |
| stream | spectrum samples, the decoded cover, the reloaded theme and appearance | latest-value cell, overwritten, read by the shell at paint | no |

Linux analogy: an OS callback is an interrupt top half (sets a flag or rings a doorbell); the driver thread is the bottom half; a `Message` is a syscall; the event loop `select` is epoll; `Effect::After` is timerfd; cells are shared memory; the shell's frame clock is the compositor's vsync; the registry is a static set of built-in drivers.

## Registry

The driver set is static: audio, macOS, library, config. `runtime::registry` holds one hand-written `DriverRow` per driver (driver name, thread name, platform) and matches exhaustively over the driver name, so a new driver fails to compile until its row is filled. Supervision defaults are kernel data (`Supervision::standard`): audio restarts up to 3 times in 60 s, library once in 60 s, then each degrades with a toast; config degrades with a toast; macOS degrades silently. The typed `Ports` (one port per driver, each with its congestion flag) is hand-written too. Wiring (which drivers start on this platform, which to join at drain) and spawn (the driver thread's and its job worker's names) read the row; the port's send reads only the driver's status in `model.drivers`; nothing else knows a driver. Adding a driver is the checklist in §4.10.

## Cells

A cell holds one latest value: a writer overwrites, the shell reads at paint, nothing queues (§12.2).

- `triple_buffer` (single producer, single consumer, wait-free) for spectrum samples (`SpectrumTap`) and, inside the audio crate, for each voice's envelope `Order` from the driver to the callback;
- atomics for scalars (congestion flags, the library overflow flag, and inside the audio crate each voice's `EnvelopeReadout` and `SeekTarget`);
- `arc-swap` for the theme, the appearance and the decoded cover: `latest_channels()` returns the senders (`publish` overwrites and rings), the receivers (`take` swaps the value out) and one bounded(1) doorbell the loop selects on.

The displayed playhead is not a cell: the kernel holds the anchor `Playhead { offset, started_at, speed }` and the shell computes the position at paint. The audio engine reports a new anchor only after an action (start, play, pause, seek, speed, track change).

## Loop, frames and backpressure

One `select` over input, the mailbox and the cell doorbell, with one deadline: the earliest of the kernel timers, the frame clock and the pending repaint. After a wake the loop gathers input first, then the mailbox, then the doorbell; the ready items form one batch, and a batch paints once. Timers fire after the batch, then congestion is settled and shell effects are handed over.

The pending repaint is `Repaint { Settled, Now, NextFrame }`: input raises it to `Now` (paint at once), a fact or a doorbell to `NextFrame` (wait for the 33 ms grid since the last paint), `Settled` paints only when the shell's own `frame_due` has passed. The frame sources (animation and a running cover fade, spectrum, progress bar, clock, sleep countdown) each live in its component's widgets module (`frame_due` fns); when nothing moves the loop blocks with no deadline.

Every channel that carries traffic is bounded. The loop only `try_send`s to drivers; a full port drops the command and raises the port's congestion flag; a driver that finds the mailbox full raises its flag before it blocks. After each batch a raised flag with no open episode becomes one `DriverEvent::Full` and one toast. No cycle can deadlock: the loop only `try_send`s, drivers only `send`.

Nothing records dropped commands, join or restart failures: the runtime keeps no trace. A debugging record is future devtools (§6.4).
