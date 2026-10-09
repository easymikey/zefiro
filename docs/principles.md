# Principles — why zefiro is shaped this way

This file says why. The rules themselves, with every name, signature and limit, live once in `docs/conventions.md`; the structure (crates, threads, message path) is in `docs/architecture.md`. When this file and the rulebook seem to disagree, the rulebook wins and this file is stale.

## The Elm Architecture

zefiro is a terminal music player shaped as The Elm Architecture (TEA): one `Model` holds all app state, one `Message` type is the only input, one `update` decides, and what should happen outside comes back as data (`Effect`s). The view is a function of the model.

Why:

- **One place decides.** Two paths to the same state are a bug class this project has hit more than once. With a single `update`, every behaviour is a row in a table: a (state, message) pair and its result.
- **Behaviour is testable without the world.** Because `update` takes the time as an argument and returns effects as values, kernel tests need no sound device, no file system, no clock and no mocks; they call the production `update` and assert on the model and the effects.
- **Refusal is a fact, not a guess.** A message the current state cannot take is reported as unhandled and changes nothing, so the runtime knows when not to repaint and the tests can assert the refusal directly.
- **Atomic steps.** Follow-up messages are drained inside the same `update` call, so no frame ever paints a half-applied model.

The same shape repeats below the kernel: every part that reacts to messages is a machine with one transition, including each driver's top machine. One form means a reader learns it once, and a new input source is one new message variant.

## Functional core, imperative shell

The kernel is pure: no IO, no clock, no threads, no environment, no channels. Everything impure sits at the edge, in drivers and the runtime, and does no deciding.

Why:

- **Decisions stay cheap to change.** A pure decision is a function with a table test; changing it never needs a device or a thread to verify.
- **Impurity is small and dumb.** A driver's impure step only performs an effect it was handed and reports what happened. Retries, coalescing, debounce and staleness are decisions, so they live in pure machines and are tested as tables.
- **Time and IO are data.** The kernel gets `Moment`s, drivers get timestamps with their commands, and results come back as messages. Nothing reads a clock where a decision is made.
- **Drivers are like OS drivers, not actors.** A request goes in, interrupt-driven events come out, and drivers never call each other. The runtime owns every thread and channel, the way an operating system owns queues and threads for its drivers.

## Layers

Crates form one direction of dependency: the pure kernel at the bottom, the source adapters (audio, library, macOS, config) above it, the runtime that wires them, the pure terminal view (widgets) beside it, terminal IO above the view, and the binary at the top.

Why:

- **A layer cannot leak upward.** The kernel never learns about CoreAudio, files or terminal escape codes; the view never learns about threads.
- **Each crate has one boundary.** Inner code works on parsed values; parsing and validating happen once, where data enters. Inner code never re-checks.
- **The composition root is the only place that knows the concrete world.** Below it, nothing knows it runs on real hardware, which is what lets tests plug fakes over the same real channels.

## Events like an operating system

The kernel receives only decisions and domain facts. How a driver does its work stays inside the driver; values that only matter when painted (spectrum, decoded cover, reloaded theme) bypass the kernel through latest-value cells. Motion state is per component: the shell owns the clock, the paint and the component state instances; the step logic is in widgets.

Why: the kernel is never flooded, a burst of input becomes one batch and one paint, and nothing is sent to the kernel only because time passed.

## Performance stance

Performance is a property of the design, not of tuning:

- **Do nothing when nothing changes.** An idle player blocks every thread and uses no CPU; frames are painted only while something on screen moves.
- **No per-frame waste.** A message that changes nothing visible paints nothing and allocates nothing; rows borrow precomputed text; images are encoded once per change.
- **Realtime paths never wait.** The audio callback never locks, allocates, frees, blocks or sends: it trades through wait-free rings, cells and atomics. OS callbacks never lock, allocate or block; they set a flag or ring a doorbell.
- **Measure on request.** Performance is reasoned from the code in reviews and measured only when the user asks, so routine work is never held up by noisy benchmarks.

## What we deliberately do not use

Typestate for the terminal (RAII is enough), trait-object plugins for drivers (a compile-time choice until a second real backend exists on one target), lenses or optics (slices and `Parts` cover it), and a second mechanism for repeated inputs beside effects (Elm-style subscriptions): effects start everything.

## Enforcement

A rule that a tool can check is held by a clippy lint; a rule that needs judgement is checked in reviews. `docs/conventions.md` marks each rule with which of the two holds it.
