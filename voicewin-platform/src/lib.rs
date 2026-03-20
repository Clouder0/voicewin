#[cfg(all(not(windows), not(target_os = "macos")))]
pub mod linux;
#[cfg(any(test, windows, target_os = "macos"))]
mod screenshot;
pub mod test;

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;
