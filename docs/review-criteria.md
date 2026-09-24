# Review criteria

Every review of a crate or block uses this list. Output: one file of rows ranked H/M/L, each row `path:line`, the criterion number, the defect and the fix; no praise; then a task list for the fix agent, each task ≤ 30 minutes. A reviewer reads, never edits.

Crate order (edges only point left, no cycles):

```
kernel  ←  config / library / audio / macos  ←  raster / runtime  ←  widgets  ←  terminal  ←  sifr
```

`terminal` never depends on `runtime`; `runtime` never depends on `widgets`, `raster` or `terminal`.

## Function level (FP-like)

1. Pure by default: the output depends only on the arguments. No clock, file system, channels, threads, environment or globals below the roots.
2. Total: every input has an answer. `Option`/`Result` instead of `unwrap`/`expect`/`panic!`/slice indexing/`unreachable!` in `src/`.
3. Expression style: a function returns a value built from `match`/`if let`/iterator chains; `let` over `let mut`; `Option`/`Result` combinators over hand-written control flow.
4. Atomic: ≤ 60 lines, ≤ 3 parameters, cognitive complexity ≤ 15, nesting ≤ 3, one concern per function; files ≤ 800 lines.
5. Exhaustive matches: no `_ =>` on our own enums; a new variant fails compilation wherever it matters.
6. No magic values: numbers and glyphs live in `impl Default` or a named constant owned by the component that uses them.
7. Names: full words, no abbreviations (`bg`, `cfg`, `msg`, `idx`, `pos`, `buf`, `err` …), no `maybe`, no `Ui*`, no `get_`/`set_`, no `x_to_y` functions, no `*Kind/*Type/*Info/*Data/*Spec/*Manager/*Helper`, no placeholder parameter names (`data`, `value`, `options`, `w`, `n`). Types are nouns, errors `*Error`, driver faults `*Failure`, overlays `*Overlay` with UPPERCASE titles.
8. No `bool` parameters: a two-variant enum instead, with no `is_*` methods on it.
9. No comments except a one-line `// SAFETY:`. No `#[allow]`, no `#[expect]`. No `super::` anywhere, no glob imports.

## Data modelling

10. ADTs: no `bool + Option`, `Option<Option<_>>`, `Vec + usize` pairs; fields that can contradict each other merge into one enum; illegal states are unrepresentable.
11. Parse, don't validate: validate once at the boundary, return a type that cannot be wrong afterwards; inner code never re-checks.
12. Newtypes at boundaries: units and identities get their own type (`Cells`, `Pixels`, `TrackIndex`, `Percent`, `Hex`); a bare integer in geometry is a defect; every `as` cast sits in a named conversion.
13. Parameter structs only for real value bundles; otherwise a method on the receiver.
14. Errors are data: `thiserror`, `#[error]` user text on every variant, `#[source]` chains, context fields, no `String` payloads, no `Box<dyn Error>`, no `anyhow`, one error enum per layer, `From` only between adjacent layers. No swallowed `let _ =` on writes, no `unwrap_or_default` hiding an error.

## Architecture

15. Layers: dependencies follow the crate order above. No cycles between modules inside a crate; a module's fan-in/fan-out stays ≤ 3 × the crate median, or the review names why.
16. Law of Demeter: below the roots (`update`, the screen root, `from_model`-style constructors, boot) no function takes `&Model`; each function receives exactly its slice.
17. Calc/effect split: every decision is a pure function with tests; the action is a separate function with no logic.
18. Effects as data, one interpreter: kernel returns `Cmd`/`Effect`; runtime interprets every one of them as a lookup table without decisions; the terminal shell only performs terminal IO. No second place matches on `Cmd`/`Effect`; no shell decision that the kernel could make.
19. No loops through IO for our own changes: a value changed by a message is applied to `Model` in that `update`; persisting it is a side effect; watchers exist only for external edits and never re-apply what the app just wrote.
20. Derived state lives next to its source: caches keyed by an explicit generation, owned by the crate that computes them (raster/widgets own pixel resources), never synced field by field per frame from the shell.
21. Components: public-field struct literal, borrows the kernel payload, owns its constants, exposes `painted(&self, screen) -> Rect`; leaves never import roots.
22. Public API minimal: everything not needed by a downstream crate is `pub(crate)` or private. No API or source file that exists only for tests. No re-exports or shims for compatibility; delete, never deprecate.
23. No extra abstractions, dependencies or crate features.

## Correctness

24. Error paths, edge cases (empty library, zero-size terminal, missing file, bad config, device gone), panics.
25. Threads and channels: races, shutdown order, a driver that dies or never answers, blocking on a full or closed channel, signal handling, RAII restoring terminal state.

## Performance (read the code; no benches)

26. A message with no visual change: no draw, no allocation.
27. A burst of keys or messages is drained in a batch and produces exactly one draw.
28. Spectrum frames never queue and never wake the loop.
29. Playlist rows borrow precomputed display text; no `String` per row per frame; no deep clones where `Arc::clone` does.
30. Idle: every thread blocked on read/select, 0 % CPU.
31. Pixel components memoized by (props, config generation); re-encode only on change.

## Tests

32. Private functions tested in `#[cfg(test)] mod tests` at the bottom of their file; public contract in `tests/unit/<area>.rs`; rstest cases and insta snapshots; fixtures on disk.
33. Behaviour is covered, not lines; every new snapshot is meaningful.
34. Runtime tests are thin and use real channels; no sound device is needed. A contract test behind `#[ignore]` because of hardware is a defect in the seam, not in the test.
35. Refactors leave snapshots unchanged; a behaviour change comes with a new snapshot and its reason.

## Events and drivers (see `docs/architecture.md`, Events)

36. Three event classes, one path each: only facts reach `update`, as a `Message` through the bounded mailbox; driver internals never leave the driver thread; streams go into latest-value cells, never a queue. No message exists only because time passed.
37. Cells are lock-free: `triple_buffer` for sample streams, atomics for scalars, `arc-swap` for large rare values. No `Mutex`, allocation or blocking call on a real-time path (audio callback, OS top half); a top half only sets a flag or `try_send`s a bounded(1) doorbell.
38. Kernel timers (`Effect::After`) only for decisions (`Timer::Mark`, sleep, toast expiry, restart backoff); a timer or message whose only purpose is to move pixels is a defect.
39. Frames only while something moves: every `frame_due` source is a pure function of (layout, anchor, now), never a sliding deadline; spectrum frames only while it is on screen and playing or decaying; when nothing moves the loop blocks with no deadline.
40. Every driver has a pure `step(state, input) -> (state, facts)` tested as a table, plus a thin IO loop (select, step, send) with no decision in it.
41. A driver receives `Outbox<ItsFact>` and sends only its own fact enum; no driver builds `kernel::Message` or holds a raw mailbox sender; the shell receives only its cells.
42. Traits only at the hardware edge: one narrow trait per kind of IO, methods take and return data, static dispatch through generics, never `dyn`; fakes plug in over the same real channels.
43. One `runtime::registry` row per driver declares its thread, placement, inbox capacity, fact type, cells and supervision strategy (`Restart`/`Backoff`/`Degrade`/`Fatal`); driver wiring outside its row, or a supervision decision outside the kernel, is a defect.
44. Congestion: every channel is bounded; the loop never blocks on a driver (coalesce idempotent commands, `try_send`, `Full` → `Dropped` trace and the row's congestion flag); a driver raises its flag before it blocks on a full mailbox; one episode yields exactly one `DriverMessage::Congested` and one toast.
