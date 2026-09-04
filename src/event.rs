/// A power or session transition delivered by [`crate::PowerWatch`].
///
/// Events are **transitions only**. Starting a watcher does not emit the
/// current lock/sleep state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerEvent {
    /// The system is going to sleep.
    Suspend,
    /// The system has finished waking.
    Resume,
    /// The session screen was locked.
    ScreenLocked,
    /// The session screen was unlocked.
    ScreenUnlocked,
    /// The system is shutting down.
    ///
    /// Best-effort: emitted on Linux via logind. Not available through the
    /// IOKit power API used on macOS.
    Shutdown,
}
