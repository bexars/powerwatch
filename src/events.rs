use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use flume::r#async::RecvStream;
use futures_core::Stream;

use crate::event::PowerEvent;

/// Event receiver from [`crate::PowerWatch::start`].
///
/// Blocking (`recv`, `try_recv`) and async (`recv_async`, [`Stream`]) APIs share
/// the same channel. No extra runtime is required.
pub struct Events {
    rx: flume::Receiver<PowerEvent>,
    stream: RecvStream<'static, PowerEvent>,
}

impl Events {
    pub(crate) fn from_flume(rx: flume::Receiver<PowerEvent>) -> Self {
        let stream = rx.clone().into_stream();
        Self { rx, stream }
    }

    /// Block until the next event, or until the watcher is dropped.
    pub fn recv(&self) -> Result<PowerEvent, RecvError> {
        self.rx.recv().map_err(|_| RecvError)
    }

    /// Receive the next event without blocking.
    pub fn try_recv(&self) -> Result<PowerEvent, TryRecvError> {
        self.rx.try_recv().map_err(|err| match err {
            flume::TryRecvError::Empty => TryRecvError::Empty,
            flume::TryRecvError::Disconnected => TryRecvError::Disconnected,
        })
    }

    /// Wait for the next event asynchronously.
    ///
    /// Works with any executor; this crate does not depend on Tokio.
    pub async fn recv_async(&self) -> Result<PowerEvent, RecvError> {
        self.rx.recv_async().await.map_err(|_| RecvError)
    }
}

impl Stream for Events {
    type Item = PowerEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().stream).poll_next(cx)
    }
}

/// An error returned from [`Events::recv`] when the channel is disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecvError;

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("receiving on a closed channel")
    }
}

impl std::error::Error for RecvError {}

/// An error returned from [`Events::try_recv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    /// No event is available yet.
    Empty,
    /// The watcher was dropped; no further events will arrive.
    Disconnected,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("receiving on an empty channel"),
            Self::Disconnected => f.write_str("receiving on a closed channel"),
        }
    }
}

impl std::error::Error for TryRecvError {}
