use crate::error::Error;
use crate::events::Events;
use crate::platform;

/// Handle that keeps OS power/session watching alive.
///
/// Keep this value until you no longer want events. Dropping it unsubscribes
/// that watcher; the last live `PowerWatch` in the process tears down the
/// shared OS runtime.
///
/// [`PowerWatch::start`] is safe to call from any thread. Multiple instances
/// in one process share a single platform runtime and each get their own
/// channel.
#[must_use = "dropping PowerWatch stops listening for events"]
pub struct PowerWatch {
    _imp: platform::Watch,
}

impl PowerWatch {
    /// Start watching OS power and session events.
    ///
    /// Returns only after the platform backend has registered (or failed).
    /// Each call gets a dedicated [`Events`]; there is no process-global
    /// event channel.
    ///
    /// `start` itself is synchronous. The returned [`Events`] can be used from
    /// a blocking thread (`recv` / `try_recv`) or awaited (`recv_async` /
    /// [`futures_core::Stream`]) without this crate starting a runtime.
    pub fn start() -> Result<(Self, Events), Error> {
        let (imp, rx) = platform::start()?;
        Ok((Self { _imp: imp }, Events::from_flume(rx)))
    }
}
