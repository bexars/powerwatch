//! Linux backend (logind / zbus) — not implemented yet.

use std::sync::mpsc::Receiver;

use crate::error::Error;
use crate::event::PowerEvent;

pub(crate) struct Watch;

pub(crate) fn start() -> Result<(Watch, Receiver<PowerEvent>), Error> {
    Err(Error::unsupported())
}
