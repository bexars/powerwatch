# Copilot plan: `powerwatch` Rust library

Build a small Rust library that watches OS power/session events and emits them on a channel. It must be safe to start from **any thread**. Do **not** clone `bexars/psp`. That crate is wrong for library use.

## Goal

`PowerWatch::start()` spawns whatever OS wait mechanism is required, returns a `Receiver<PowerEvent>`, and does not depend on the caller pumping a GUI event loop.

Events:

- `Suspend` — system is going to sleep
- `Resume` — system woke
- `ScreenLocked`
- `ScreenUnlocked`
- `Shutdown` — best effort (Linux yes; macOS/Windows only if cheap and correct)

## Public API (do not change the shape)

```rust
use std::sync::mpsc::Receiver; 

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    Suspend,
    Resume,
    ScreenLocked,
    ScreenUnlocked,
    Shutdown,
}

pub struct PowerWatch { /* private */ }

impl PowerWatch {
    /// Safe to call from any thread. Owns a private worker until dropped.
    pub fn start() -> Result<(Self, Receiver<PowerEvent>), Error>;
}

impl Drop for PowerWatch {
    fn drop(&mut self);
}

#[derive(Debug)]
pub struct Error { /* Display + std::error::Error */ }
```

Rules:

- Each `start()` gets its **own** channel. No process-global `OnceLock` sender.
- `Drop` must stop the worker (quit the run loop / `PostQuitMessage` / drop dbus connection and join with a timeout).
- Keep `PowerWatch` alive to keep listening. Document that.
- No AppKit, tao, winit, or NSWorkspace.

## Crate layout

```
powerwatch/
  Cargo.toml
  src/lib.rs          // re-exports
  src/event.rs
  src/error.rs
  src/watch.rs        // PowerWatch + start() dispatch
  src/platform.rs     // cfg re-export
  src/platform/macos.rs
  src/platform/windows.rs
  src/platform/linux.rs
  src/platform/unsupported.rs
  examples/watch.rs   // print events; try_recv + 100ms sleep on the caller side
```

`Cargo.toml`:

- edition 2021
- `std::mpsc` 
- Windows: `windows` crate, only the features you use
- macOS: `core-foundation` + IOKit bindings (`objc2-io-kit` or equivalent). Prefer a dispatch queue if the binding supports `IONotificationPortSetDispatchQueue`; otherwise a private CFRunLoop thread.
- Linux: `zbus` **blocking** API. Do not spin up a private tokio runtime unless there is no blocking alternative.

## Platform requirements

### macOS (`src/platform/macos.rs`)

Private worker thread named `powerwatch-macos`.

Sleep/wake:

1. `IORegisterForSystemPower`
2. Attach the notification port to **this thread’s** CFRunLoop, or set a GCD queue owned by the library
3. In the IOKit callback:
   - `kIOMessageCanSystemSleep` → `IOAllowPowerChange` **immediately**, then optionally send nothing (or treat as Suspend only if you also get WillSleep; prefer sending `Suspend` on `kIOMessageSystemWillSleep` only)
   - `kIOMessageSystemWillSleep` → `IOAllowPowerChange` **immediately**, then `send(Suspend)`
   - `kIOMessageSystemHasPoweredOn` / `kIOMessageSystemWillPowerOn` → send `Resume` on HasPoweredOn
4. Never do blocking work before `IOAllowPowerChange`
5. On stop: `IODeregisterForSystemPower`, destroy the port, `IOServiceClose`, `CFRunLoopStop`

Lock/unlock:

- Darwin `notify_register_dispatch` for `com.apple.screenIsLocked` and `com.apple.screenIsUnlocked`
- Not NSDistributedNotificationCenter, not NSWorkspace

Hold `root_port`, notifier, port, notify tokens until `Drop`.

### Windows (`src/platform/windows.rs`)

Private worker thread named `powerwatch-windows`.

1. Register a window class
2. Create a **message-only** window (`HWND_MESSAGE`)
3. `WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)`
4. Run `GetMessage` / `TranslateMessage` / `DispatchMessage` until quit
5. `wndproc`:
   - `WM_POWERBROADCAST` + `PBT_APMSUSPEND` → `Suspend`
   - `PBT_APMRESUMESUSPEND` and/or `PBT_APMRESUMEAUTOMATIC` → `Resume` (dedupe if both fire)
   - `WM_WTSSESSION_CHANGE` + `WTS_SESSION_LOCK` / `WTS_SESSION_UNLOCK`
6. `Drop` posts `WM_QUIT` to that thread/window and joins

Do not create a window and return. No pump = no events.

### Linux (`src/platform/linux.rs`)

Private worker thread named `powerwatch-linux`.

Use logind on the system bus, blocking zbus:

- `org.freedesktop.login1.Manager` `PrepareForSleep(start: bool)`  
  `true` → `Suspend`, `false` → `Resume`
- `PrepareForShutdown(start: bool)` → `Shutdown` when `start == true`
- Session `LockedHint` property changes → lock/unlock  
  Ignore the first snapshot if it is “current state,” not a transition (document this)

If D-Bus / logind is missing, return a clear error from `start()`.

### Other OS

`start()` returns `Error::Unsupported`.

## Concurrency / lifecycle

```
PowerWatch::start()
  creates channel
  spawns platform worker
  worker registers OS hooks
  worker signals “ready” or “failed” over a oneshot
  start() returns (PowerWatch, rx) only after ready
```

If registration fails, join the thread and return `Err`.

`Drop`:

1. set an atomic stop flag
2. platform-specific wakeup (runloop stop / WM_QUIT / drop connection)
3. join (e.g. 2s timeout; if timeout, log and leak rather than deadlock—pick one and document)

## Example `examples/watch.rs`

```rust
let (_watch, rx) = powerwatch::PowerWatch::start()?;
loop {
    match rx.try_recv() {
        Ok(ev) => println!("{ev:?}"),
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => break,
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
}
```

Do **not** put `try_recv` on the OS worker thread.

## Tests

- Channel: start a fake/internal test helper if you extract `send` behind a trait; otherwise skip OS tests in CI.
- Compile `examples/watch.rs` on each target.
- Manual macOS check: binary name `power` is fine. After sleep, `pmset -g log` must **not** contain `power timed out(30000 ms)`.
- Manual: lock screen, unlock, sleep, wake; each prints once.

## Explicit bans (reject the patch if present)

- `NSWorkspace`, `NSWorkspaceWillSleepNotification`, `NSApplication`
- Registering observers on the caller thread
- Global static channel
- Windows HWND without `GetMessage` loop
- `IOAllowPowerChange` omitted or called after user work
- Creating a tokio runtime just for Linux
- `unwrap()` on OS API failure in library paths

## Implementation order

1. `event` + `error` + `PowerWatch` skeleton + unsupported backend
2. Linux (simplest real backend)
3. Windows message thread
4. macOS IOKit + Darwin notify (highest risk; ack first)
5. Example + README with the `pmset` warning

## README (short)

- What events mean
- `PowerWatch` must be kept alive
- Thread-safe `start()`
- Platform table
- macOS: we ack sleep immediately; we do not delay sleep
- Linux needs logind/D-Bus

Ship a compiling crate with `start()` / `Drop` working on the current OS first, then fill the other two backends. Do not publish a design that requires the embedder to run a GUI loop.