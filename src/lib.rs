//! Controller discovery and the abstract pad.
//!
//! This crate sits below both consumers described in
//! [ADR-0006](../../../docs/adr/0006-input-and-controller-mapping.md): the
//! launcher, which needs a controller to drive the interface, and
//! `apex-emu`, which needs one to drive a core. It is a **system** crate
//! for that reason, not an emulation one.
//!
//! # Scope
//!
//! This is the first stage: an event source, device tracking, and the
//! abstract pad's state. Mapping, persistence, the layout wizard and the
//! RetroPad projection are later stages and are deliberately absent.
//!
//! # Two boundaries this crate exists to hold
//!
//! **It does not depend on the launcher.** ADR-0006 decision 2 describes the
//! launcher-facing projection in terms of `focus::Action`, but that type
//! belongs to the launcher's focus model. This crate emits its own
//! vocabulary and the launcher converts, so `apex-emu` can use the same
//! crate without inheriting a UI type.
//!
//! **It does not expose its backend.** ADR-0006 decision 4's amendment says
//! the abstraction is "deliberately not gilrs-shaped, so falling back to
//! `evdev` changes one decision and leaves the other nine standing." That is
//! only true if something enforces it, which is what [`EventSource`] is for.
//! Nothing outside `gilrs_backend` names a gilrs type.
//!
//! # Polling, and the absence of threads
//!
//! [`EventSource::poll`] is pull-based and this crate owns no thread. Both
//! consumers already have a loop with different timing needs — the launcher a
//! UI event loop, `apex-emu` a frame loop — and a thread here would make
//! each inherit the other's scheduling.

mod device;
mod error;
mod intent;
mod pad;
mod source;
mod synthetic;

#[cfg(feature = "gilrs-backend")]
mod gilrs_backend;

pub use device::{DeviceId, DeviceInfo, MappingSource, ModelId};
pub use error::Error;
pub use intent::{Intent, Intents};
pub use pad::{Axis, Button, Devices, Pad};
pub use source::{Event, EventSource};
pub use synthetic::Synthetic;

#[cfg(feature = "gilrs-backend")]
pub use gilrs_backend::GilrsSource;
