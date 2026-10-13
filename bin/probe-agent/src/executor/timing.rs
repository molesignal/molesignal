// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where an attempt spends its time, as `TimingBreakdown` in `probe.proto`
//! defines it: every phase is a duration of its own, summed over all the
//! connections the attempt opens, and reported whole or not at all.

use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::protocol::v1::TimingBreakdown;

const PHASES: usize = 4;

/// The phases on the way to a reply, in the order an attempt goes through them.
#[derive(Clone, Copy)]
pub(super) enum Phase {
    Dns,
    Connect,
    Tls,
    FirstByte,
}

/// The phase timings of one attempt.
///
/// Clones share their readings, so a clock can follow a connection into the
/// client that owns it.
#[derive(Clone, Default)]
pub(super) struct PhaseClock {
    /// All the time spent so far in each phase the attempt has been through.
    readings: Arc<Mutex<[Option<Duration>; PHASES]>>,
}

impl PhaseClock {
    /// Starts timing `phase`. The time counts once the returned guard is dropped.
    pub fn enter(&self, phase: Phase) -> Running {
        Running {
            clock: self.clone(),
            phase,
            started: Instant::now(),
        }
    }

    /// Runs `work` inside `phase`. The time counts whether `work` finishes,
    /// fails, or is dropped half way by a timeout.
    pub async fn during<T>(&self, phase: Phase, work: impl Future<Output = T>) -> T {
        let _running = self.enter(phase);
        work.await
    }

    /// The breakdown to report for an attempt that took `total`.
    pub fn breakdown(&self, total: Duration) -> TimingBreakdown {
        let readings = *self.readings();
        let timed = |phase: Phase| readings[phase as usize].map(micros);

        TimingBreakdown {
            dns_micros: timed(Phase::Dns),
            connect_micros: timed(Phase::Connect),
            tls_micros: timed(Phase::Tls),
            first_byte_micros: timed(Phase::FirstByte),
            total_micros: Some(micros(total)),
        }
    }

    fn spend(&self, phase: Phase, spent: Duration) {
        let mut readings = self.readings();
        let so_far = readings[phase as usize].unwrap_or_default();
        readings[phase as usize] = Some(so_far + spent);
    }

    fn readings(&self) -> MutexGuard<'_, [Option<Duration>; PHASES]> {
        // The lock only guards plain assignments, so a poisoned one still holds sound readings.
        self.readings.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A phase in progress. Dropping it counts the time since it started.
pub(super) struct Running {
    clock: PhaseClock,
    phase: Phase,
    started: Instant,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.clock.spend(self.phase, self.started.elapsed());
    }
}

/// A connection that times the wait for its reply.
///
/// The wait starts when the connection is wrapped, which is when it is ready
/// to carry a request. It ends with the first read to complete: the first
/// bytes of the reply, or the connection closing before any.
pub(super) struct AwaitingReply<S> {
    stream: S,
    waiting: Option<Running>,
}

impl<S> AwaitingReply<S> {
    pub fn new(stream: S, phases: &PhaseClock) -> Self {
        Self {
            stream,
            waiting: Some(phases.enter(Phase::FirstByte)),
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for AwaitingReply<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let read = Pin::new(&mut self.stream).poll_read(cx, buffer);

        if read.is_ready() {
            self.waiting = None;
        }

        read
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for AwaitingReply<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
