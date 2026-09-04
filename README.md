# powerwatch

Watch OS power and session events on a Rust channel. `PowerWatch::start()` is safe to call from any thread and does **not** need a GUI event loop.

Keep the returned `PowerWatch` alive. Dropping it unsubscribes; the last instance in the process stops the shared OS runtime.

## Events

| Event | Meaning |
| --- | --- |
| `Suspend` | System is going to sleep |
| `Resume` | System finished waking |
| `ScreenLocked` | Session screen locked |
| `ScreenUnlocked` | Session screen unlocked |
| `Shutdown` | System is shutting down (best-effort) |

Events are **transitions only**. Starting a watcher does not emit the current lock or sleep state.

## Example

```rust
use powerwatch::TryRecvError;

let (_watch, events) = powerwatch::PowerWatch::start()?;
loop {
    match events.try_recv() {
        Ok(ev) => println!("{ev:?}"),
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => break,
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
}
```

Do not drain the channel on the OS worker. `try_recv` belongs on your thread.

`Events` also supports async without a crate-provided runtime: `recv_async` and `futures_core::Stream`. This crate does not depend on Tokio. See `examples/watch_tokio.rs` (`cargo run --example watch_tokio`).

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut task = tokio::spawn(async {
        let (_watch, events) = powerwatch::PowerWatch::start()?;
        loop {
            match events.recv_async().await {
                Ok(ev) => println!("{ev:?}"),
                Err(_) => break,
            }
        }
        Ok::<_, powerwatch::Error>(())
    });

    tokio::select! {
        result = &mut task => {
            result??;
        }
        _ = tokio::signal::ctrl_c() => {
            task.abort();
        }
    }
    Ok(())
}
```

## Platforms

| OS | Sleep / wake | Lock / unlock | Shutdown |
| --- | --- | --- | --- |
| macOS | IOKit `IORegisterForSystemPower` | Darwin `notify` (`com.apple.screenIsLocked` / `Unlocked`) | not available (IOKit power API does not deliver it) |
| Linux | stub | stub | stub |
| Windows | stub | stub | stub |
| other | `Error` | | |

Multiple `PowerWatch` instances in one process share a single platform runtime and each get their own channel.

### macOS

Sleep is acknowledged with `IOAllowPowerChange` immediately. This crate does **not** delay sleep. After a sleep cycle, `pmset -g log` should not contain `<program> timed out(30000 ms)` for your process.

### Linux

Will require logind on the system D-Bus (not implemented yet).
