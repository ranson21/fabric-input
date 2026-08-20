//! The launcher-facing projection: what a control *means* to an interface.
//!
//! ADR-0006 decision 2 defines two projections over one abstract pad. This is
//! the first — navigation intent, consumed by the launcher and by every system
//! screen. The other, RetroPad, arrives with `apex-emu`.
//!
//! # Why this is not `focus::Action`
//!
//! It is deliberately a separate type from the launcher's `focus::Action`,
//! even though the variants line up. `apex-emu` will use this crate too,
//! and a crate that both consumers depend on must not carry one consumer's UI
//! vocabulary. The launcher converts, in one place, cheaply.
//!
//! # Why confirm accepts two positions
//!
//! Spike I1 measured the primary action button reporting as **east** on
//! Nintendo-layout pads and **south** on Xbox-layout ones — not a database
//! fault, but the layouts genuinely disagreeing about which position means
//! "confirm". Until a layout is known (ADR-0006 decision 8, stage 4), the only
//! choice that works on every pad is to accept both.
//!
//! The cost is that neither position is free to mean "back", which is why
//! [`Button::Select`] carries it for now. That is an interim, and the layout
//! wizard is what replaces it.

use crate::pad::{Axis, Button, Pad};

/// A navigation intent, already independent of which control produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    TabPrev,
    TabNext,
}

impl Intent {
    /// The name the launcher's UI already uses for this action.
    ///
    /// Strings rather than a shared enum because `AppWindow.slint` funnels
    /// every key press through one `navigate(string)` callback, and
    /// `navigation::parse_action` turns those names into actions. Reusing that
    /// path means controller input reaches the focus model by exactly the
    /// route a key press does, which is the convention `main.rs` already
    /// follows for mouse clicks.
    pub fn as_name(self) -> &'static str {
        match self {
            Intent::Up => "up",
            Intent::Down => "down",
            Intent::Left => "left",
            Intent::Right => "right",
            Intent::Select => "select",
            Intent::Back => "back",
            Intent::TabPrev => "tab_prev",
            Intent::TabNext => "tab_next",
        }
    }
}

/// Past this much deflection a stick counts as a direction held.
///
/// Generous on purpose. A stick has to be pushed deliberately to move focus,
/// and I1 measured every pad resting at centre, so there is room between
/// "resting" and "meant it".
const STICK_THRESHOLD: f32 = 0.6;

/// Below this, a stick has returned far enough to be pushed again.
///
/// Lower than the trigger point, so a stick wavering around one value cannot
/// emit a stream of intents. Without the gap, resting *at* the threshold
/// produces exactly the runaway the wizard's neutral check exists to prevent.
const STICK_RELEASE: f32 = 0.4;

/// Turns pad state into intents, remembering enough to fire on edges only.
///
/// Intents are emitted when a control becomes active, never while it stays
/// active. Holding a direction therefore moves focus once. Auto-repeat is a
/// later stage: it needs a clock, and this stays a pure function of state so
/// it can be tested against recorded streams without one.
#[derive(Debug, Default)]
pub struct Intents {
    held: Vec<(Button, bool)>,
    stick: [i8; 2],
}

impl Intents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intents implied by the difference between the last pad state and this
    /// one, in a stable order.
    pub fn update(&mut self, pad: &Pad) -> Vec<Intent> {
        let mut out = Vec::new();

        for (button, intent) in BUTTONS {
            let now = pad.pressed(button);
            let was = self.was(button);
            if now && !was {
                out.push(intent);
            }
            self.set(button, now);
        }

        // A stick is a direction only when pushed past the threshold, and
        // re-arms once it comes back. Both axes are independent so a diagonal
        // push emits both, which the focus model handles as two moves.
        //
        // The pair is (negative, positive) for each axis, and the vertical one
        // is not the way round it looks.
        //
        // gilrs normalises axes itself and negates LeftStickY, RightStickY and
        // DPadY wherever `IS_Y_AXIS_REVERSED` — which is every platform this
        // runs on. So evdev's "positive is down" has already been undone by
        // the time a value arrives here: a raw ABS_Y of 0, the stick pushed
        // up, reaches this loop as +1.
        //
        // It used to read (Up, Down), on a comment that said positive was down
        // "matching evdev". True of evdev, not of gilrs, and the effect was a
        // d-pad with its vertical axis inverted — pressing up moved down.
        // Found on a Mega Drive style USB pad whose d-pad is reported as
        // ABS_X/ABS_Y rather than as a hat, so it arrives through this path
        // rather than the hat one.
        for (slot, (axis, neg, pos)) in [
            (Axis::LeftX, Intent::Left, Intent::Right),
            (Axis::LeftY, Intent::Down, Intent::Up),
        ]
        .into_iter()
        .enumerate()
        {
            let value = pad.axis(axis);
            let was = self.stick[slot];
            let now = if value >= STICK_THRESHOLD {
                1
            } else if value <= -STICK_THRESHOLD {
                -1
            } else if value.abs() <= STICK_RELEASE {
                0
            } else {
                was
            };
            if now != was {
                match now {
                    1 => out.push(pos),
                    -1 => out.push(neg),
                    _ => {}
                }
                self.stick[slot] = now;
            }
        }

        out
    }

    fn was(&self, button: Button) -> bool {
        self.held
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, held)| *held)
            .unwrap_or(false)
    }

    fn set(&mut self, button: Button, held: bool) {
        if let Some(entry) = self.held.iter_mut().find(|(b, _)| *b == button) {
            entry.1 = held;
        } else {
            self.held.push((button, held));
        }
    }
}

/// Buttons that carry an intent, and what each means.
///
/// `FaceSouth` and `FaceEast` both confirm — see the module docs. `Guide` is
/// deliberately absent: I1 found the N64 pad reporting Start as `Mode`, so a
/// Guide binding cannot be trusted before a layout is known, which is the
/// same evidence that moved ADR-0006 decision 10 to a deliberate hold.
///
/// `Home` has no controller binding yet for the same reason, and the keyboard
/// keeps it — decision 9 requires that a controller is never the only way in.
///
/// `Start` is absent too, and deliberately: it is half of decision 10's
/// reserved exit binding, and confirm is already covered by both face
/// positions, so binding it would add ambiguity and no capability.
///
/// # Pads without two shoulders
///
/// A Mega Drive-style pad has **one** shoulder; some retro pads have none.
/// Such a pad gets one tab direction, or neither, and that asymmetry cannot
/// be resolved here: which of the two a single shoulder maps to is decided by
/// the controller database, and this layer does not know the pad's shape.
/// Knowing it is exactly what ADR-0006 decision 8's layout wizard is for.
///
/// It is a limitation rather than a trap. **Tab switching never depends on a
/// shoulder**: `Up` from the content reaches the tab bar and `Left`/`Right`
/// move along it, on every pad, including one with no shoulders at all.
/// Binding the triggers as well widens the coverage without making any pad
/// worse.
const BUTTONS: [(Button, Intent); 11] = [
    (Button::DpadUp, Intent::Up),
    (Button::DpadDown, Intent::Down),
    (Button::DpadLeft, Intent::Left),
    (Button::DpadRight, Intent::Right),
    (Button::FaceSouth, Intent::Select),
    (Button::FaceEast, Intent::Select),
    (Button::Select, Intent::Back),
    (Button::ShoulderLeft, Intent::TabPrev),
    (Button::ShoulderRight, Intent::TabNext),
    // Triggers carry the same intents, so a pad whose single shoulder the
    // database happens to map as a trigger still switches tabs. See the note
    // on pads that do not have two shoulders.
    (Button::TriggerLeft, Intent::TabPrev),
    (Button::TriggerRight, Intent::TabNext),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceId, DeviceInfo, MappingSource, ModelId};
    use crate::pad::Devices;
    use crate::source::Event;

    fn devices() -> Devices {
        let mut d = Devices::new();
        d.apply(Event::Connected {
            id: DeviceId(0),
            info: DeviceInfo {
                name: "pad".into(),
                model: ModelId([0; 16]),
                mapping: MappingSource::Sdl,
            },
        });
        d
    }

    fn press(d: &mut Devices, button: Button, pressed: bool) {
        d.apply(Event::Button {
            id: DeviceId(0),
            button,
            pressed,
        });
    }

    fn axis(d: &mut Devices, axis: Axis, value: f32) {
        d.apply(Event::Axis {
            id: DeviceId(0),
            axis,
            value,
        });
    }

    #[test]
    fn a_button_fires_once_on_press_not_while_held() {
        let mut d = devices();
        let mut i = Intents::new();
        press(&mut d, Button::DpadDown, true);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Down]);
        // Still held, second poll: nothing new.
        assert!(i.update(d.pad(DeviceId(0)).unwrap()).is_empty());
        press(&mut d, Button::DpadDown, false);
        assert!(i.update(d.pad(DeviceId(0)).unwrap()).is_empty());
        press(&mut d, Button::DpadDown, true);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Down]);
    }

    #[test]
    fn both_face_positions_confirm_because_layouts_disagree() {
        // I1 measured the primary button at east on Nintendo-layout pads and
        // south on Xbox-layout ones. Until the layout is known, both.
        for button in [Button::FaceSouth, Button::FaceEast] {
            let mut d = devices();
            let mut i = Intents::new();
            press(&mut d, button, true);
            assert_eq!(
                i.update(d.pad(DeviceId(0)).unwrap()),
                vec![Intent::Select],
                "{button:?} should confirm"
            );
        }
    }

    #[test]
    fn the_guide_button_carries_no_intent() {
        // I1 found the N64 pad reporting Start as Mode, so Guide is not
        // trustworthy before a layout is known.
        let mut d = devices();
        let mut i = Intents::new();
        press(&mut d, Button::Guide, true);
        assert!(i.update(d.pad(DeviceId(0)).unwrap()).is_empty());
    }

    #[test]
    fn shoulders_switch_tabs() {
        let mut d = devices();
        let mut i = Intents::new();
        press(&mut d, Button::ShoulderRight, true);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::TabNext]);
        press(&mut d, Button::ShoulderLeft, true);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::TabPrev]);
    }

    #[test]
    fn a_stick_pushed_and_released_fires_once_each_way() {
        let mut d = devices();
        let mut i = Intents::new();
        axis(&mut d, Axis::LeftX, 0.9);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Right]);
        // Held past the threshold: no repeat.
        axis(&mut d, Axis::LeftX, 0.95);
        assert!(i.update(d.pad(DeviceId(0)).unwrap()).is_empty());
        // Back to centre re-arms, then push the other way.
        axis(&mut d, Axis::LeftX, 0.0);
        assert!(i.update(d.pad(DeviceId(0)).unwrap()).is_empty());
        axis(&mut d, Axis::LeftX, -0.9);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Left]);
    }

    #[test]
    fn a_stick_resting_between_the_thresholds_does_not_chatter() {
        // The hysteresis gap exists for exactly this: a worn stick sitting
        // near the trigger point must not emit a stream of intents.
        let mut d = devices();
        let mut i = Intents::new();
        // Positive is up: gilrs has already undone evdev's sign by here.
        axis(&mut d, Axis::LeftY, 0.7);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Up]);
        for v in [0.55, 0.5, 0.58, 0.52] {
            axis(&mut d, Axis::LeftY, v);
            assert!(
                i.update(d.pad(DeviceId(0)).unwrap()).is_empty(),
                "value {v} should not re-fire"
            );
        }
    }

    #[test]
    fn triggers_switch_tabs_too_so_a_one_shoulder_pad_has_a_chance() {
        // A Mega Drive-style pad has one shoulder, and which of the pair the
        // database maps it to is not ours to choose. Binding the triggers as
        // well widens coverage without changing anything for a pad that has
        // both shoulders.
        for (button, expected) in [
            (Button::TriggerLeft, Intent::TabPrev),
            (Button::TriggerRight, Intent::TabNext),
        ] {
            let mut d = devices();
            let mut i = Intents::new();
            press(&mut d, button, true);
            assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![expected]);
        }
    }

    #[test]
    fn a_pad_with_no_shoulders_can_still_reach_every_tab() {
        // The limitation is asymmetric convenience, not a dead end: Up
        // reaches the tab bar and Left/Right move along it. This asserts the
        // intents exist on a pad that only has a d-pad, which is what makes
        // the shoulder bindings optional rather than load-bearing.
        let mut d = devices();
        let mut i = Intents::new();
        for (button, expected) in [
            (Button::DpadUp, Intent::Up),
            (Button::DpadLeft, Intent::Left),
            (Button::DpadRight, Intent::Right),
        ] {
            press(&mut d, button, true);
            assert_eq!(
                i.update(d.pad(DeviceId(0)).unwrap()),
                vec![expected],
                "{button:?} must work without any shoulder present"
            );
            press(&mut d, button, false);
            i.update(d.pad(DeviceId(0)).unwrap());
        }
    }

    #[test]
    fn every_intent_has_a_name_the_launcher_parses() {
        // The names must match navigation::parse_action, which is a separate
        // crate and cannot be checked by the compiler.
        let expected = [
            "up", "down", "left", "right", "select", "back", "tab_prev", "tab_next",
        ];
        for intent in [
            Intent::Up,
            Intent::Down,
            Intent::Left,
            Intent::Right,
            Intent::Select,
            Intent::Back,
            Intent::TabPrev,
            Intent::TabNext,
        ] {
            assert!(
                expected.contains(&intent.as_name()),
                "{intent:?} produced an unexpected name"
            );
        }
    }

    /// Up is up.
    ///
    /// This is the whole of the Sega pad bug. gilrs negates LeftStickY,
    /// RightStickY and DPadY wherever `IS_Y_AXIS_REVERSED`, which is every
    /// platform this runs on — so a stick pushed up arrives POSITIVE, and a
    /// table written for evdev's convention turned it into Down.
    ///
    /// A pad whose d-pad is reported as ABS_X/ABS_Y rather than as a hat comes
    /// through this path, which is why a Mega Drive style pad found it and the
    /// recognised pads did not.
    #[test]
    fn pushing_up_means_up() {
        let mut d = devices();
        let mut i = Intents::new();

        axis(&mut d, Axis::LeftY, 1.0);
        assert_eq!(
            i.update(d.pad(DeviceId(0)).unwrap()),
            vec![Intent::Up],
            "a positive Y is the stick pushed up"
        );

        axis(&mut d, Axis::LeftY, 0.0);
        let _ = i.update(d.pad(DeviceId(0)).unwrap());

        axis(&mut d, Axis::LeftY, -1.0);
        assert_eq!(
            i.update(d.pad(DeviceId(0)).unwrap()),
            vec![Intent::Down],
            "a negative Y is the stick pulled down"
        );
    }

    /// And the horizontal axis is untouched by that fix.
    ///
    /// The first attempt corrected the vertical direction inside the match
    /// shared by both axes, which inverted left and right as well. The
    /// existing horizontal test caught it; this states the expectation rather
    /// than relying on that.
    #[test]
    fn pushing_right_still_means_right() {
        let mut d = devices();
        let mut i = Intents::new();

        axis(&mut d, Axis::LeftX, 1.0);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Right]);

        axis(&mut d, Axis::LeftX, 0.0);
        let _ = i.update(d.pad(DeviceId(0)).unwrap());

        axis(&mut d, Axis::LeftX, -1.0);
        assert_eq!(i.update(d.pad(DeviceId(0)).unwrap()), vec![Intent::Left]);
    }
}
