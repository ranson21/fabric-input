//! The crate's error type.

/// Something went wrong reaching the input devices.
///
/// Deliberately does **not** wrap a backend error type. Returning
/// `gilrs::Error` would put gilrs in this crate's public API, which is
/// precisely the coupling ADR-0006 decision 4's amendment says must not
/// exist — a caller would then have to change if the backend did, and the
/// claim that swapping it "leaves the other nine decisions standing" would
/// stop being true.
///
/// The backend's own message is preserved as text, because it is the only
/// part a human needs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No input backend could be started. On Linux this is usually a
    /// permissions problem reading `/dev/input/event*`.
    #[error("could not start the input backend: {0}")]
    Backend(String),
}
