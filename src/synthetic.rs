//! An event source backed by a script rather than by hardware.
//!
//! Not a testing convenience bolted on afterwards. ADR-0006 decision 8
//! requires the layout wizard to be "a pure function over an event stream" so
//! that its failure modes — a drifting stick auto-filling every prompt, a pad
//! missing a control the wizard asks for, a d-pad arriving as buttons rather
//! than as axes — are tested in CI without hardware. That is only possible if
//! a stream can be supplied, which is what this is for.
//!
//! Spike I1 captured those streams from real controllers, so the fixtures a
//! later stage replays here are recordings rather than guesses.

use crate::source::{Event, EventSource};

/// Replays a fixed list of events, once, in order.
#[derive(Debug, Default)]
pub struct Synthetic {
    queue: std::collections::VecDeque<Event>,
}

impl Synthetic {
    pub fn new(events: impl IntoIterator<Item = Event>) -> Self {
        Self {
            queue: events.into_iter().collect(),
        }
    }

    /// Queue more events, so a test can interleave input with assertions
    /// rather than declaring the whole stream up front.
    pub fn push(&mut self, event: Event) {
        self.queue.push_back(event);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl EventSource for Synthetic {
    fn poll(&mut self) -> Option<Event> {
        self.queue.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceId, DeviceInfo, MappingSource, ModelId};
    use crate::pad::{Button, Devices};

    fn connected(id: u32) -> Event {
        Event::Connected {
            id: DeviceId(id),
            info: DeviceInfo {
                name: "test pad".into(),
                model: ModelId([1; 16]),
                mapping: MappingSource::None,
            },
        }
    }

    #[test]
    fn events_arrive_in_order_and_then_stop() {
        let mut s = Synthetic::new([
            connected(1),
            Event::Button {
                id: DeviceId(1),
                button: Button::Start,
                pressed: true,
            },
        ]);

        assert!(matches!(s.poll(), Some(Event::Connected { .. })));
        assert!(matches!(s.poll(), Some(Event::Button { .. })));
        assert_eq!(s.poll(), None);
        assert!(s.is_empty());
    }

    #[test]
    fn drain_into_applies_everything_pending_not_just_the_first() {
        // The reason `drain_into` exists: a caller that polls once per frame
        // is indistinguishable from input lag.
        let mut s = Synthetic::new([
            connected(1),
            Event::Button {
                id: DeviceId(1),
                button: Button::FaceSouth,
                pressed: true,
            },
            Event::Button {
                id: DeviceId(1),
                button: Button::FaceEast,
                pressed: true,
            },
        ]);
        let mut devices = Devices::new();
        s.drain_into(&mut devices);

        let pad = devices.pad(DeviceId(1)).expect("device tracked");
        assert!(pad.pressed(Button::FaceSouth));
        assert!(pad.pressed(Button::FaceEast));
        assert!(s.is_empty());
    }

    #[test]
    fn pushing_mid_stream_works_so_tests_can_interleave() {
        let mut s = Synthetic::new([connected(1)]);
        let mut devices = Devices::new();
        s.drain_into(&mut devices);
        assert_eq!(devices.connected(), vec![DeviceId(1)]);

        s.push(Event::Disconnected { id: DeviceId(1) });
        s.drain_into(&mut devices);
        assert!(devices.is_empty());
    }
}
