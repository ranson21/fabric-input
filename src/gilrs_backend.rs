//! The gilrs-backed event source.
//!
//! **This is the only module that names a gilrs type.** ADR-0006 decision 4
//! selected gilrs but recorded that the abstraction is "deliberately not
//! gilrs-shaped, so falling back to `evdev` changes one decision and leaves
//! the other nine standing". Keeping the dependency to one file is what makes
//! that true rather than aspirational.

use std::collections::{HashMap, VecDeque};

use crate::device::{DeviceId, DeviceInfo, MappingSource, ModelId};
use crate::error::Error;
use crate::pad::{Axis, Button};
use crate::source::{Event, EventSource};

/// Deflection past which a hat axis counts as a direction held.
///
/// A d-pad reported as an axis has no natural threshold, and a value that
/// merely drifts must not read as a press. Half travel is well clear of the
/// noise spike I1 measured — every pad rested at centre — while still being
/// reached by any real press, since hats report full deflection or nothing.
const HAT_THRESHOLD: f32 = 0.5;

pub struct GilrsSource {
    inner: gilrs::Gilrs,
    /// gilrs' own id is opaque, so instances are numbered here. The map is
    /// keyed on gilrs' id, which spike I1 measured is reused when a pad
    /// reconnects — so a replugged pad keeps its [`DeviceId`], which is what
    /// ADR-0006 decision 6 relies on for player slots.
    ids: HashMap<gilrs::GamepadId, DeviceId>,
    next_id: u32,
    /// One incoming event can produce more than one outgoing event: a hat
    /// axis crossing centre releases one direction and presses another.
    pending: VecDeque<Event>,
    /// Last hat position per device, so only genuine changes are emitted.
    hats: HashMap<(DeviceId, bool), i8>,
}

impl GilrsSource {
    pub fn new() -> Result<Self, Error> {
        let inner = gilrs::Gilrs::new().map_err(|e| Error::Backend(e.to_string()))?;
        let mut source = Self {
            inner,
            ids: HashMap::new(),
            next_id: 0,
            pending: VecDeque::new(),
            hats: HashMap::new(),
        };
        // Devices already attached at start-up never produce a Connected
        // event, so without this they would be invisible until unplugged.
        let existing: Vec<_> = source
            .inner
            .gamepads()
            .map(|(id, pad)| (id, describe(&pad)))
            .collect();
        for (gid, info) in existing {
            let id = source.assign(gid);
            source.pending.push_back(Event::Connected { id, info });
        }
        Ok(source)
    }

    fn assign(&mut self, gid: gilrs::GamepadId) -> DeviceId {
        if let Some(id) = self.ids.get(&gid) {
            return *id;
        }
        let id = DeviceId(self.next_id);
        self.next_id += 1;
        self.ids.insert(gid, id);
        id
    }

    /// Turn a hat axis into the direction presses and releases it implies.
    fn hat(&mut self, id: DeviceId, horizontal: bool, value: f32) {
        let now: i8 = if value > HAT_THRESHOLD {
            1
        } else if value < -HAT_THRESHOLD {
            -1
        } else {
            0
        };
        let key = (id, horizontal);
        let was = self.hats.insert(key, now).unwrap_or(0);
        if was == now {
            return;
        }
        let (neg, pos) = if horizontal {
            (Button::DpadLeft, Button::DpadRight)
        } else {
            // Positive is UP. gilrs negates DPadY wherever
            // `IS_Y_AXIS_REVERSED`, which is every platform this runs on, so
            // evdev's convention has already been undone by the time a value
            // reaches here. The same mistake was in the stick path and had the
            // same effect: up moved down.
            (Button::DpadDown, Button::DpadUp)
        };
        let button_for = |v: i8| match v {
            -1 => Some(neg),
            1 => Some(pos),
            _ => None,
        };
        if let Some(button) = button_for(was) {
            self.pending.push_back(Event::Button {
                id,
                button,
                pressed: false,
            });
        }
        if let Some(button) = button_for(now) {
            self.pending.push_back(Event::Button {
                id,
                button,
                pressed: true,
            });
        }
    }
}

impl EventSource for GilrsSource {
    fn poll(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            let raw = self.inner.next_event()?;
            let id = self.assign(raw.id);
            match raw.event {
                gilrs::EventType::Connected => {
                    let info = describe(&self.inner.gamepad(raw.id));
                    self.pending.push_back(Event::Connected { id, info });
                }
                gilrs::EventType::Disconnected => {
                    // Drop hat state so a reconnecting pad cannot inherit a
                    // direction it is no longer holding.
                    self.hats.remove(&(id, true));
                    self.hats.remove(&(id, false));
                    self.pending.push_back(Event::Disconnected { id });
                }
                gilrs::EventType::ButtonPressed(b, _) => {
                    if let Some(button) = button(b) {
                        self.pending.push_back(Event::Button {
                            id,
                            button,
                            pressed: true,
                        });
                    }
                }
                gilrs::EventType::ButtonReleased(b, _) => {
                    if let Some(button) = button(b) {
                        self.pending.push_back(Event::Button {
                            id,
                            button,
                            pressed: false,
                        });
                    }
                }
                gilrs::EventType::AxisChanged(a, value, _) => match a {
                    gilrs::Axis::DPadX => self.hat(id, true, value),
                    gilrs::Axis::DPadY => self.hat(id, false, value),
                    other => {
                        if let Some(axis) = axis(other) {
                            self.pending.push_back(Event::Axis { id, axis, value });
                        }
                    }
                },
                // Analog button travel and force-feedback completions carry
                // nothing this crate models yet.
                _ => {}
            }
        }
    }
}

fn describe(pad: &gilrs::Gamepad<'_>) -> DeviceInfo {
    DeviceInfo {
        name: pad.name().to_string(),
        model: ModelId(pad.uuid()),
        mapping: match pad.mapping_source() {
            gilrs::MappingSource::SdlMappings => MappingSource::Sdl,
            gilrs::MappingSource::Driver => MappingSource::Driver,
            gilrs::MappingSource::None => MappingSource::None,
        },
    }
}

/// gilrs' button vocabulary onto this crate's positional one.
///
/// `C` and `Z` are dropped: they are Mega Drive-era extras with no position
/// on the abstract pad, and inventing one would be a guess. A layout that
/// needs them is a stage-4 problem, where the pad's type is known.
fn button(b: gilrs::Button) -> Option<Button> {
    use gilrs::Button as G;
    Some(match b {
        G::South => Button::FaceSouth,
        G::East => Button::FaceEast,
        G::North => Button::FaceNorth,
        G::West => Button::FaceWest,
        G::LeftTrigger => Button::ShoulderLeft,
        G::RightTrigger => Button::ShoulderRight,
        G::LeftTrigger2 => Button::TriggerLeft,
        G::RightTrigger2 => Button::TriggerRight,
        G::LeftThumb => Button::ThumbLeft,
        G::RightThumb => Button::ThumbRight,
        G::Start => Button::Start,
        G::Select => Button::Select,
        G::Mode => Button::Guide,
        G::DPadUp => Button::DpadUp,
        G::DPadDown => Button::DpadDown,
        G::DPadLeft => Button::DpadLeft,
        G::DPadRight => Button::DpadRight,
        G::C | G::Z | G::Unknown => return None,
    })
}

fn axis(a: gilrs::Axis) -> Option<Axis> {
    use gilrs::Axis as G;
    Some(match a {
        G::LeftStickX => Axis::LeftX,
        G::LeftStickY => Axis::LeftY,
        G::RightStickX => Axis::RightX,
        G::RightStickY => Axis::RightY,
        G::LeftZ => Axis::TriggerLeft,
        G::RightZ => Axis::TriggerRight,
        // Handled by the hat path before this is reached.
        G::DPadX | G::DPadY | G::Unknown => return None,
    })
}
