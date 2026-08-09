//! Device identity.
//!
//! ADR-0006 decision 6 keys two different things on two different identities,
//! and conflating them is the cause of pain point 2 — the reason player one
//! and player two swap when pads are replugged.
//!
//! - A **mapping** is keyed on [`ModelId`], so every copy of a controller
//!   model shares one mapping and a second identical pad needs no setup.
//! - A **player slot** is keyed on [`DeviceId`], the runtime instance, so the
//!   system can tell two identical pads apart while they are both connected.

use std::fmt;

/// A connected device, for as long as it stays connected.
///
/// Spike I1 measured that a pad unplugged and replugged into the same socket
/// keeps this value — five pads, five reconnects, no exceptions. A pad moved
/// to a *different* port was not tested, and ADR-0006 records that gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u32);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "device{}", self.0)
    }
}

/// A controller *model*, as the SDL GUID: bus, vendor, product, version.
///
/// Deliberately not the device's name. Spike I1 found two of six pads
/// mislabelled by the bundled database — an N64 pad reporting as "Ipega PG
/// 9099", a NES pad reporting as an Xbox 360 controller because it spoofs
/// Microsoft's vendor and product id — so a name is not a stable key for
/// anything, and is only ever shown to a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelId(pub [u8; 16]);

impl ModelId {
    /// The SDL GUID form: lowercase hex, no separators. This is the string a
    /// mapping file is named after, so it must not change casing or spacing.
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Where a device's current mapping came from, if anywhere.
///
/// Spike I1's finding is why this is recorded rather than assumed: a device
/// being *recognised* says nothing about the mapping being *usable*. Six of
/// six pads reported [`MappingSource::Sdl`], and two of them were mapped to
/// the wrong controller entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingSource {
    /// Matched an entry in the bundled controller database.
    Sdl,
    /// Normalised by a kernel driver before userspace saw it.
    Driver,
    /// Unrecognised. Needs the layout wizard.
    None,
}

/// What is known about a connected device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// For display only. Never a key — see [`ModelId`].
    pub name: String,
    pub model: ModelId,
    pub mapping: MappingSource,
}
