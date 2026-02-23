mod resample;

#[cfg(any(windows, target_os = "macos"))]
mod recorder;

#[cfg(any(windows, target_os = "macos"))]
pub use recorder::{AudioCaptureError, AudioRecorder, CapturedAudio};

#[cfg(any(windows, target_os = "macos"))]
pub use recorder::AudioInputDeviceInfo;
