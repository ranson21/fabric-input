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
    /// A button with no position on the abstract pad, carried by the code the
    /// kernel gave it.
    ///
    /// Two things arrive here. **Mega Drive-era extras** — the six-button
    /// pad's `C` and the N64's `Z` — which have no abstract position and
    /// which inventing one for would be a guess. And **every button of a pad
    /// with no mapping at all**, which is the case that matters: an
    /// unrecognised pad used to produce no events whatsoever, so a frontend
    /// could not offer to set it up, because the screen offering to do so
    /// could not itself be operated.
    ///
    /// The code is stable for a given device and is not comparable between
    /// devices. It is meant to be recorded by something that has asked a
    /// person what the pad is, and read back later — not interpreted.
    Other(u32),
}

impl Button {
    /// Every *positioned* variant, so consumers can enumerate without matching
    /// by hand.
    ///
    /// [`Button::Other`] is deliberately absent: it carries a device's own
    /// code and there is no finite set of those to list. Anything walking this
    /// array is asking about the abstract pad, which is exactly the question
    /// `Other` has no answer to.
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

    /// Where this button lives in a [`Pad`]'s array, or `None` for
    /// [`Button::Other`], which has no seat in it.
    ///
    /// **Was `unwrap_or(0)`, and that was the whole bug.** Seat zero is
    /// `FaceSouth`, so every unmapped button an `Other` carried landed on
    /// Confirm: an unrecognised pad had seventeen buttons that all read as A,
    /// which is worse than the silence `Other` was added to end. Returning
    /// nothing forces the caller to say what it means instead.
    fn index(self) -> Option<usize> {
        Button::ALL.iter().position(|b| *b == self)
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
        // Every variant is in ALL, and unlike `Button` there is no open-ended
        // one that could fall through.
        Axis::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }
}

/// The state of one abstract pad.
///
/// Two stores, because there are two kinds of button. The array is the
/// abstract pad — a fixed set of positions, and the only thing a game or a
/// menu should ever ask about. `others` is whatever the device itself
/// reported and nothing has a name for; it is open-ended, so it cannot be an
/// array, and mixing the two is what put every unmapped press on Confirm.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pad {
    buttons: [bool; Button::ALL.len()],
    /// Raw codes currently held. Small: a pad has a handful of buttons and
    /// only the ones with no position land here, so a scan beats a hash.
    others: Vec<u32>,
    axes: [f32; Axis::ALL.len()],
}

impl Pad {
    pub fn pressed(&self, button: Button) -> bool {
        match button {
            Button::Other(code) => self.others.contains(&code),
            positioned => positioned.index().is_some_and(|seat| self.buttons[seat]),
        }
    }

    /// Every button currently held, positioned and raw alike.
    ///
    /// For the one caller that legitimately wants "what is the person
    /// pressing *right now*" rather than "is this particular button down":
    /// a frontend walking somebody through naming their pad's buttons. That
    /// screen cannot ask about a button by name, because finding out the name
    /// is why it exists.
    pub fn down(&self) -> Vec<Button> {
        let mut out: Vec<Button> = Button::ALL
            .iter()
            .copied()
            .filter(|b| self.pressed(*b))
            .collect();
        out.extend(self.others.iter().copied().map(Button::Other));
        out
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
        !self.buttons.iter().any(|b| *b)
            && self.others.is_empty()
            && self.axes.iter().all(|a| a.abs() < f32::EPSILON)
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
                    match button {
                        Button::Other(code) => {
                            pad.others.retain(|c| *c != code);
                            if pressed {
                                pad.others.push(code);
                            }
                        }
                        // `index` is `Some` for every other variant, and the
                        // `else` is unreachable rather than ignored — a new
                        // open-ended variant added without a home here should
                        // be loud, not silently dropped.
                        positioned => match positioned.index() {
                            Some(seat) => pad.buttons[seat] = pressed,
                            None => tracing::error!(
                                ?positioned,
                                "a button with no seat and no raw code; dropped"
                            ),
                        },
                    }
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

    /// A button with no abstract position keeps its own identity.
    ///
    /// The case this exists for is a pad with no mapping at all, which used to
    /// produce no events whatsoever — so a frontend could not offer to set it
    /// up, because the screen offering to do so could not be operated by the
    /// pad it was asking about.
    #[test]
    fn an_unpositioned_button_is_told_apart_by_its_code() {
        assert_ne!(Button::Other(304), Button::Other(305));
        assert_eq!(Button::Other(304), Button::Other(304));

        // And it is not one of the positioned ones, which is what keeps
        // anything matching on the abstract pad honest.
        assert!(!Button::ALL.contains(&Button::Other(304)));
        assert_eq!(Button::ALL.len(), 17, "the abstract pad grew a button");
    }
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

    /// An unmapped button must not read as Confirm.
    ///
    /// The regression this file was changed for. `Button::Other` was added so
    /// an unrecognised pad emits something rather than nothing, and it was
    /// then folded into the same array as the positioned buttons through an
    /// `index()` that answered `0` for it — seat zero being `FaceSouth`. So
    /// every button on an unrecognised pad set A, which is not a partial
    /// mapping but a wrong one: a frontend asking somebody to press Start
    /// would have been told they pressed A, and saved that.
    #[test]
    fn a_raw_button_does_not_land_on_face_south() {
        let mut devices = Devices::new();
        devices.apply(Event::Connected {
            id: DeviceId(1),
            info: DeviceInfo {
                name: "unrecognised pad".into(),
                model: ModelId([7; 16]),
                mapping: MappingSource::None,
            },
        });

        devices.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Other(304),
            pressed: true,
        });

        let pad = devices.pad(DeviceId(1)).expect("connected");
        assert!(pad.pressed(Button::Other(304)), "the raw press was lost");
        assert!(
            !pad.pressed(Button::FaceSouth),
            "an unmapped button was reported as A"
        );
        assert!(
            !pad.is_neutral(),
            "something is held and the pad says it is not"
        );

        // And a different code is a different button, not the same seat again.
        assert!(!pad.pressed(Button::Other(305)));
    }

    /// Releasing a raw button releases that one.
    #[test]
    fn raw_buttons_release_independently() {
        let mut devices = Devices::new();
        devices.apply(Event::Connected {
            id: DeviceId(1),
            info: DeviceInfo {
                name: "unrecognised pad".into(),
                model: ModelId([7; 16]),
                mapping: MappingSource::None,
            },
        });
        for code in [304, 305, 306] {
            devices.apply(Event::Button {
                id: DeviceId(1),
                button: Button::Other(code),
                pressed: true,
            });
        }
        devices.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Other(305),
            pressed: false,
        });

        let pad = devices.pad(DeviceId(1)).expect("connected");
        assert!(pad.pressed(Button::Other(304)));
        assert!(!pad.pressed(Button::Other(305)));
        assert!(pad.pressed(Button::Other(306)));

        // Held twice is held once. gilrs repeats a press across a
        // reconnection and a pad that grew a button each time would report
        // the same code as several.
        devices.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Other(304),
            pressed: true,
        });
        let pad = devices.pad(DeviceId(1)).expect("connected");
        assert_eq!(
            pad.down()
                .iter()
                .filter(|b| **b == Button::Other(304))
                .count(),
            1
        );
    }

    /// `down` is what a mapping walk reads, and it has to see both kinds.
    #[test]
    fn down_reports_positioned_and_raw_together() {
        let mut devices = Devices::new();
        devices.apply(Event::Connected {
            id: DeviceId(1),
            info: DeviceInfo {
                name: "half-known pad".into(),
                model: ModelId([9; 16]),
                mapping: MappingSource::Sdl,
            },
        });
        devices.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Start,
            pressed: true,
        });
        devices.apply(Event::Button {
            id: DeviceId(1),
            button: Button::Other(310),
            pressed: true,
        });

        let down = devices.pad(DeviceId(1)).expect("connected").down();
        assert_eq!(down.len(), 2, "{down:?}");
        assert!(down.contains(&Button::Start));
        assert!(down.contains(&Button::Other(310)));
    }
}
