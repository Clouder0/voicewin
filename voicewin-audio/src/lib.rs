pub mod resample;

// Capture will be implemented behind OS-specific backends.
#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::{AudioCaptureError, AudioRecorder, CapturedAudio};
