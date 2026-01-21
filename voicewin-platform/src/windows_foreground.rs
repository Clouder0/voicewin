// Windows foreground app detection.
// Kept behind cfg(windows) at the module level in windows.rs.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use voicewin_core::types::{AppIdentity, WindowTitle};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

pub fn get_foreground_app_identity() -> anyhow::Result<AppIdentity> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(AppIdentity::new());
        }

        // Window title
        let title = get_window_title(hwnd).ok();

        // Process ID
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let exe_path = get_process_exe_path(pid).ok();
        let process_name = exe_path
            .as_ref()
            .and_then(|p| std::path::Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()));

        let mut app = AppIdentity::new();
        if let Some(path) = exe_path {
            app = app.with_exe_path(path);
        }
        if let Some(name) = process_name {
            app = app.with_process_name(name);
        }
        if let Some(t) = title {
            app.window_title = Some(WindowTitle(t));
        }

        Ok(app)
    }
}

fn get_window_title(hwnd: HWND) -> anyhow::Result<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return Ok(String::new());
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        let copied = copied.max(0) as usize;
        buf.truncate(copied);
        Ok(String::from_utf16_lossy(&buf))
    }
}

fn get_process_exe_path(pid: u32) -> anyhow::Result<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;

        // Windows recommends MAX_PATH as a starting point but paths can be longer.
        let mut buf = vec![0u16; 4096];

        // K32GetModuleFileNameExW requires PROCESS_QUERY_INFORMATION | PROCESS_VM_READ normally,
        // but on modern Windows PROCESS_QUERY_LIMITED_INFORMATION is often sufficient.
        // If it fails, we still return an error.
        let len = K32GetModuleFileNameExW(Some(handle), None, &mut buf) as usize;
        let _ = CloseHandle(handle);

        if len == 0 {
            return Err(anyhow::anyhow!("failed to query exe path"));
        }

        buf.truncate(len);
        let os = OsString::from_wide(&buf);
        Ok(os.to_string_lossy().to_string())
    }
}
