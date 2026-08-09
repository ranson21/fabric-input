//! The abstract pad, and the devices currently presenting one.
//!
//! ADR-0006 decision 2 resolves every physical device to one abstract pad,
//! which two consumers then project differently. This is that pad. It holds
//! state and makes no decisions about meaning.

use std::collections::HashMap;

use crate::device::{DeviceId, DeviceInfo};
use crate::source::Event;

/// A control on the abstract pad, named by **position**.
///
/// The naming is the load-bearing part. Spike I1 measured the primary action
/// button reporting as east on two independent retro pads — an N64's A at
/// `BTN_EAST`, a NES's A at `BTN_EAST` with B at `BTN_SOUTH` — across
/// different vendors and different database entries. The pads are not wrong:
/// Nintendo layouts put the primary button on the right and Xbox layouts put
/// it on the bottom, and each reports where its button physically is.
///
/// So position and meaning genuinely differ, and a name like `A` would be a
/// lie in half the cases. Which position *means* confirm is a question for
/// the layout, not for this type — see ADR-0006 decision 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    FaceSouth,
    FaceEast,
    FaceNorth,
    FaceWest,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    ShoulderLeft,
    ShoulderRight,
    TriggerLeft,
    TriggerRight,
    ThumbLeft,
    ThumbRight,
    Start,
    Select,
    /// Guide, Home, or whatever the pad calls it.
    ///
    /// Present because some pads have one, and **not** trusted as an exit
    /// binding: I1 found the N64 pad reporting Start as `Mode`, which is why
    /// ADR-0006 decision 10 was amended to a deliberate hold instead.
    Guide,
}

impl Button {
    /// Every variant, so consumers can enumerate without matching by hand.
    pub const ALL: [Button; 17] = [
        Button::FaceSouth,
        Button::FaceEast,
        Button::FaceNorth,
        Button::FaceWest,
        Button::DpadUp,
        Button::DpadDown,
        Button::DpadLeft,
        Button::DpadRight,
        Button::ShoulderLeft,
        Button::ShoulderRight,
        Button::TriggerLeft,
        Button::TriggerRight,
        Button::ThumbLeft,
        Button::ThumbRight,
        Button::Start,
        Button::Select,
        Button::Guide,
    ];

    fn index(self) -> usize {
        Button::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }
}

/// A continuous control, in `-1.0..=1.0`, or `0.0..=1.0` for triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    TriggerLeft,
    TriggerRight,
}

impl Axis {
    pub const ALL: [Axis; 6] = [
        Axis::LeftX,
        Axis::LeftY,
        Axis::RightX,
        Axis::RightY,
        Axis::TriggerLeft,
        Axis::TriggerRight,
    ];

    fn index(self) -> usize {
        Axis::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }
}

/// The state of one abstract pad.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pad {
    buttons: [bool; Button::ALL.len()],
    axes: [f32; Axis::ALL.len()],
}

impl Pad {
    pub fn pressed(&self, button: Button) -> bool {
        self.buttons[button.index()]
    }

    pub fn axis(&self, axis: Axis) -> f32 {
        self.axes[axis.index()]
    }

    /// True when nothing is held and every axis is centred.
    ///
    /// Used by the wizard to require a return to neutral between prompts —
    /// ADR-0006 decision 8's guard against a drifting stick auto-filling
    /// every binding.
    pub fn is_neutral(&self) -> bool {
        !self.buttons.iter().any(|b| *b) && self.axes.iter().all(|a| a.abs() < f32::EPSILON)
    }
}

/// Every device currently connected, and the pad each presents.
#[derive(Debug, Default)]
pub struct Devices {
    pads: HashMap<DeviceId, Pad>,
    info: HashMap<DeviceId, DeviceInfo>,
}

impl Devices {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one event into the current state.
    ///
    /// Events for a device that never connected are ignored rather than
    /// treated as an error. A source is entitled to report a control change
    /// slightly before or after its connect and disconnect, and an appliance
    /// with no shell has to keep working through that — the same reasoning as
    /// the launcher skipping an unreadable app rather than refusing to start.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Connected { id, info } => {
                tracing::info!(%id, name = %info.name, model = %info.model,
                    mapping = ?info.mapping, "device connected");
                self.info.insert(id, info);
                self.pads.insert(id, Pad::default());
            }
            Event::Disconnected { id } => {
                tracing::info!(%id, "device disconnected");
                self.info.remove(&id);
                self.pads.remove(&id);
            }
            Event::Button {
                id,
                button,
                pressed,
            } => {
                if let Some(pad) = self.pads.get_mut(&id) {
                    pad.buttons[button.index()] = pressed;
                } else {
                    tracing::debug!(%id, ?button, "button for an unknown device, ignored");
                }
            }
            Event::Axis { id, axis, value } => {
                if let Some(pad) = self.pads.get_mut(&id) {
                    pad.axes[axis.index()] = value;
                } else {
                    tracing::debug!(%id, ?axis, "axis for an unknown device, ignored");
                }
            }
        }
    }

    pub fn pad(&self, id: DeviceId) -> Option<&Pad> {
        self.pads.get(&id)
    }

    pub fn info(&self, id: DeviceId) -> Option<&DeviceInfo> {
        self.info.get(&id)
    }

    /// Connected devices, in a stable order so callers and tests agree.
    pub fn connected(&self) -> Vec<DeviceId> {
        let mut ids: Vec<_> = self.info.keys().copied().collect();
        ids.sort();
        ids
    }

    pub fn is_empty(&self) -> bool {
        self.info.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{MappingSource, ModelId};

    fn info(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.to_string(),
            model: ModelId([0xab; 16]),
            mapping: MappingSource::Sdl,
        }
    }

    fn connected(id: u32, name: &str) -> Event {
        Event::Connected {
            id: DeviceId(id),
            info: info(name),
        }
    }

    #[test]
    fn a_press_then_release_returns_the_pad_to_neutral() {
        let mut d = Devices::new();
        d.apply(connected(1, "pad"));
        assert!(d.pad(DeviceId(1)).unwrap().is_neutral());

        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::FaceSouth,
            pressed: true,
        });
        assert!(d.pad(DeviceId(1)).unwrap().pressed(Button::FaceSouth));
        assert!(!d.pad(DeviceId(1)).unwrap().is_neutral());

        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::FaceSouth,
            pressed: false,
        });
        assert!(d.pad(DeviceId(1)).unwrap().is_neutral());
    }

    #[test]
    fn buttons_are_independent() {
        let mut d = Devices::new();
        d.apply(connected(1, "pad"));
        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::FaceEast,
            pressed: true,
        });
        let pad = d.pad(DeviceId(1)).unwrap();
        assert!(pad.pressed(Button::FaceEast));
        for b in Button::ALL.into_iter().filter(|b| *b != Button::FaceEast) {
            assert!(!pad.pressed(b), "{b:?} should not be pressed");
        }
    }

    #[test]
    fn a_deflected_axis_is_not_neutral() {
        let mut d = Devices::new();
        d.apply(connected(1, "pad"));
        d.apply(Event::Axis {
            id: DeviceId(1),
            axis: Axis::LeftX,
            value: -0.8,
        });
        let pad = d.pad(DeviceId(1)).unwrap();
        assert_eq!(pad.axis(Axis::LeftX), -0.8);
        assert!(!pad.is_neutral());
        assert_eq!(pad.axis(Axis::LeftY), 0.0);
    }

    #[test]
    fn disconnecting_forgets_the_device_entirely() {
        let mut d = Devices::new();
        d.apply(connected(1, "pad"));
        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Start,
            pressed: true,
        });
        d.apply(Event::Disconnected { id: DeviceId(1) });

        assert!(d.pad(DeviceId(1)).is_none());
        assert!(d.info(DeviceId(1)).is_none());
        assert!(d.is_empty());
    }

    #[test]
    fn a_reconnecting_device_starts_neutral_rather_than_stale() {
        // Spike I1 measured that a replugged pad keeps its id, so state left
        // over from before would silently reappear as a held button.
        let mut d = Devices::new();
        d.apply(connected(1, "pad"));
        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::FaceNorth,
            pressed: true,
        });
        d.apply(Event::Disconnected { id: DeviceId(1) });
        d.apply(connected(1, "pad"));

        assert!(d.pad(DeviceId(1)).unwrap().is_neutral());
    }

    #[test]
    fn events_for_an_unknown_device_are_ignored_not_fatal() {
        let mut d = Devices::new();
        d.apply(Event::Button {
            id: DeviceId(99),
            button: Button::Start,
            pressed: true,
        });
        d.apply(Event::Axis {
            id: DeviceId(99),
            axis: Axis::LeftY,
            value: 1.0,
        });
        assert!(d.is_empty());
    }

    #[test]
    fn devices_are_tracked_separately_and_listed_in_a_stable_order() {
        let mut d = Devices::new();
        d.apply(connected(2, "second"));
        d.apply(connected(1, "first"));
        d.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Select,
            pressed: true,
        });

        assert_eq!(d.connected(), vec![DeviceId(1), DeviceId(2)]);
        assert!(d.pad(DeviceId(1)).unwrap().pressed(Button::Select));
        assert!(!d.pad(DeviceId(2)).unwrap().pressed(Button::Select));
    }

    #[test]
    fn every_button_and_axis_has_a_distinct_slot() {
        // A duplicated index would make two controls alias, which is the kind
        // of fault that presents as "the wrong button does something".
        let mut seen = std::collections::HashSet::new();
        for b in Button::ALL {
            assert!(seen.insert(b.index()), "{b:?} shares an index");
        }
        let mut seen = std::collections::HashSet::new();
        for a in Axis::ALL {
            assert!(seen.insert(a.index()), "{a:?} shares an index");
        }
    }
}
