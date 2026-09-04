//! OS power and session events on a channel, without a GUI event loop.
//!
//! [`PowerWatch::start`] is safe to call from any thread. Keep the returned
//! `PowerWatch` alive for as long as you want events.

mod error;
mod event;
mod events;
mod platform;
mod watch;

pub use error::Error;
pub use event::PowerEvent;
pub use events::{Events, RecvError, TryRecvError};
pub use watch::PowerWatch;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn start_and_drop_disconnects_channel() {
        let (watch, rx) = PowerWatch::start().expect("start");
        drop(watch);
        assert!(rx.recv().is_err());
    }

    #[test]
    fn two_instances_have_independent_channels() {
        let (a, ra) = PowerWatch::start().expect("start a");
        let (b, rb) = PowerWatch::start().expect("start b");
        drop(a);
        assert!(ra.recv().is_err());
        assert!(matches!(rb.try_recv(), Err(TryRecvError::Empty)));
        drop(b);
        assert!(rb.recv().is_err());
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn start_is_unsupported() {
        assert!(PowerWatch::start().is_err());
    }
}
