//! Where events come from, and the boundary that keeps a backend replaceable.

use crate::device::{DeviceId, DeviceInfo};
use crate::pad::{Axis, Button};

/// One change, already expressed in this crate's vocabulary.
///
/// Nothing here names a backend type. That is the point: ADR-0006 decision 4
/// records that gilrs was chosen but that "the abstraction is deliberately not
/// gilrs-shaped", so a fall back to reading `evdev` directly would replace one
/// implementation and leave everything above it untouched.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected {
        id: DeviceId,
        info: DeviceInfo,
    },
    Disconnected {
        id: DeviceId,
    },
    Button {
        id: DeviceId,
        button: Button,
        pressed: bool,
    },
    Axis {
        id: DeviceId,
        axis: Axis,
        value: f32,
    },
}

/// A source of [`Event`]s, drained by the caller.
///
/// Pull-based on purpose. Both consumers already own a loop with different
/// timing needs — the launcher a UI event loop, `apex-emu` a frame loop —
/// so a source that pushed, or that owned a thread, would make each inherit
/// the other's scheduling.
pub trait EventSource {
    /// The next pending event, or `None` when the queue is empty.
    ///
    /// Must not block. A caller is expected to drain in a loop and then get
    /// on with its frame.
    fn poll(&mut self) -> Option<Event>;

    /// Drain everything pending into `devices`.
    ///
    /// Provided because every caller wants exactly this, and because doing it
    /// by hand invites draining only one event per frame — which looks like
    /// input lag and is tedious to diagnose.
    fn drain_into(&mut self, devices: &mut crate::pad::Devices) {
        while let Some(event) = self.poll() {
            devices.apply(event);
        }
    }
}
