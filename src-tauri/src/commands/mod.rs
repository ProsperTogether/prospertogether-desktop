pub mod auth;
pub mod devices;
pub mod evidence_bundle;
pub mod frames;
pub mod recording;
pub mod recordings;
pub mod setup;
pub mod transcription;
pub mod updater;
pub mod upload;
pub mod window_capture;

/// Backward-compatible alias module name used by older drafts.
pub mod evidence {
    pub use super::evidence_bundle::*;
}
