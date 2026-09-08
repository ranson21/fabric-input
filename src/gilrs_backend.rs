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

/// Corrected SDL mappings for pads the bundled database gets wrong.
///
/// One entry so far. A generic DragonRise board (USB `0079:0011`), sold in
/// Mega Drive and Saturn shaped cases, is matched by the SDL database as a
/// "Sega Saturn" pad. That entry is correct SDL — it maps the d-pad as two
/// half-axis pairs, `dpup:-a1, dpdown:+a1, dpleft:-a0, dpright:+a0` — and
/// gilrs collapses each pair into ONE full-range analog button and drops the
/// other half. Measured on the pad: only `DPadUp` and `DPadRight` ever arrive,
/// and pressing down or left emits **nothing at all**. Two of four directions
/// simply do not exist, which no amount of interpretation downstream can
/// recover.
///
/// So the same axes are mapped as a stick instead. gilrs then delivers them
/// across their full travel — measured at +0.991 up, -1.000 down, -0.991 left,
/// +1.000 right — and the stick path already turns deflection into directions
/// in both signs.
///
/// The button assignments are carried over from the bundled entry unchanged.
/// They are wrong for a six-button pad — C and Z both land on a shoulder — but
/// that is a different problem: no rule recovers a physical layout from a
/// mistaken one, which is what ADR-0006 section 8's wizard exists for. Fixing
/// the directions without pretending to fix the faces is the honest half.
/// **The second entry is the N64 pad ADR-0043 opens with.** Spike I1 recorded
/// "an N64 pad reported as an Ipega PG 9099"; the same house's pad still is,
/// measured 2026-09-07 — `name="Ipega PG 9099"`, GUID
/// `03000000632500007505000011010000`, `mapping=Sdl`. The bundled entry is not
/// merely the wrong *name*. Through it, a person pressing the ten buttons in
/// order gets: Start as `Guide`, A as `FaceEast`, B as `FaceSouth`, and the
/// four C buttons scattered over `Start`, `FaceNorth`, `FaceWest` and
/// `Select`. Three of ten land where a frontend would look for them.
///
/// The physical pad, read from the kernel rather than assumed — every button
/// pressed once, in a known order, against `/dev/input/event*`:
///
/// ```text
///   A  BTN_EAST b1      C-Up    BTN_TR2   b9     Z      BTN_TL b6
///   B  BTN_C    b2      C-Down  BTN_SOUTH b0     L      BTN_WEST b4
///   Start BTN_MODE b12  C-Left  BTN_NORTH b3     R      BTN_Z  b5
///   d-pad hat h0        C-Right BTN_TL2   b8     stick  a0/a1
/// ```
///
/// **The four C buttons are mapped as buttons, not as a right stick, and that
/// was measured too.** A C cluster is a direction cluster and the obvious
/// mapping is `+rightx`/`-rightx`/`+righty`/`-righty`. gilrs drops half of it,
/// exactly as the Saturn note above describes for the d-pad: holding each in
/// turn produced 44 ticks of `RightY = +1.00` and 36 of `RightX = +1.00`, and
/// **nothing whatsoever** from the two negative halves. Two of four buttons
/// would not exist.
///
/// So they take four ordinary positions, and that is a deliberate compromise
/// rather than a claim about the pad's shape: an N64 controller has ten
/// buttons, the standard vocabulary has no C cluster in it, and the shoulders
/// are the only four-of-a-kind left once the faces are spent. A C button
/// nobody can press is worse than an L button in an unexpected place, and a
/// frontend that maps positionally reaches all ten this way.
const MAPPING_OVERRIDES: &str = "\
03000000790000001100000011010000,Sega Saturn,\
a:b1,b:b2,x:b0,y:b3,\
leftshoulder:b6,lefttrigger:b7,rightshoulder:b5,righttrigger:b4,\
back:b8,start:b9,leftx:a0,lefty:a1,platform:Linux,\n\
03000000632500007505000011010000,Nintendo 64,\
a:b1,b:b0,x:b2,y:b9,\
back:b4,start:b12,\
leftshoulder:b3,rightshoulder:b8,lefttrigger:b6,righttrigger:b5,\
dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,\
leftx:a0,lefty:a1,platform:Linux,";

/// Applies [`MAPPING_OVERRIDES`], by the only route that actually wins.
///
/// gilrs inserts mappings into one table keyed by GUID, in this order at
/// build time: the builder's own `add_mappings`, then the bundled database,
/// then `SDL_GAMECONTROLLERCONFIG`. Later insertions overwrite earlier ones,
/// so anything passed to the builder is overwritten by the bundled entry that
/// it was meant to replace — measured, not assumed: the override had no effect
/// at all until it moved here.
///
/// Any existing value is kept and ours appended, so a person who has set the
/// variable for their own pad does not lose it.
fn apply_mapping_overrides() {
    let existing = std::env::var("SDL_GAMECONTROLLERCONFIG").unwrap_or_default();
    let combined = if existing.is_empty() {
        MAPPING_OVERRIDES.to_string()
    } else {
        format!("{existing}\n{MAPPING_OVERRIDES}")
    };
    // Safe here in practice and worth naming: this runs once, before any
    // gamepad thread exists, from the constructor of the only type in this
    // crate that talks to gilrs.
    std::env::set_var("SDL_GAMECONTROLLERCONFIG", combined);
}

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
        apply_mapping_overrides();
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
                gilrs::EventType::ButtonPressed(b, code) => {
                    self.pending.push_back(Event::Button {
                        id,
                        button: button(b).unwrap_or(Button::Other(code.into_u32())),
                        pressed: true,
                    });
                }
                gilrs::EventType::ButtonReleased(b, code) => {
                    self.pending.push_back(Event::Button {
                        id,
                        button: button(b).unwrap_or(Button::Other(code.into_u32())),
                        pressed: false,
                    });
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
                // Force-feedback completions and the rest carry nothing this
                // crate models yet.
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
/// `C`, `Z` and anything unrecognised return `None` here and are carried by
/// the caller as [`Button::Other`], with the code the kernel gave them. They
/// are Mega Drive-era extras with no position on the abstract pad, and
/// inventing one would still be a guess — but dropping them meant a frontend
/// that knows the pad's type could not reach them either, and an unmapped pad
/// produced no events at all. Passing the code through leaves the guess
/// undone while letting somebody who has asked a person what the pad is
/// record the answer.
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

#[cfg(test)]
mod mapping_overrides {
    use super::*;

    /// The override must be a mapping gilrs will actually parse: a GUID, a
    /// name, then comma separated bindings, with the platform gilrs matches on.
    #[test]
    fn the_override_is_well_formed() {
        for line in MAPPING_OVERRIDES.lines() {
            let mut parts = line.split(',');
            let guid = parts.next().unwrap();
            assert_eq!(guid.len(), 32, "a GUID is 32 hex characters: {guid:?}");
            assert!(guid.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(parts.next().is_some_and(|n| !n.is_empty()), "no name");
            assert!(
                line.contains("platform:Linux,"),
                "gilrs skips any mapping whose platform is not this one"
            );
        }
    }

    /// All four directions, or it does not fix what it was written for.
    ///
    /// The bundled entry binds the d-pad as half-axis pairs and gilrs loses
    /// one half of each, so this maps the same axes as a stick.
    #[test]
    fn the_override_gives_the_dpad_both_halves_of_both_axes() {
        assert!(MAPPING_OVERRIDES.contains("leftx:a0"));
        assert!(MAPPING_OVERRIDES.contains("lefty:a1"));
        for half in ["dpup:", "dpdown:", "dpleft:", "dpright:"] {
            assert!(
                !MAPPING_OVERRIDES.contains(half),
                "{half} is the binding gilrs mishandles; the point is to avoid it"
            );
        }
    }

    /// An existing value is kept. Somebody may have set this for their own pad.
    #[test]
    fn an_existing_configuration_is_not_thrown_away() {
        let theirs = "0000000000000000000000000000ffff,Their Pad,a:b0,platform:Linux,";
        std::env::set_var("SDL_GAMECONTROLLERCONFIG", theirs);
        apply_mapping_overrides();

        let now = std::env::var("SDL_GAMECONTROLLERCONFIG").unwrap();
        assert!(now.contains(theirs), "someone else's mapping was discarded");
        assert!(now.contains("03000000790000001100000011010000"));

        std::env::remove_var("SDL_GAMECONTROLLERCONFIG");
    }
}
