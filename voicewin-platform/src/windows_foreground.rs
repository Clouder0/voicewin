// Windows foreground app detection.
// Kept behind cfg(windows) at the module level in windows.rs.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use voicewin_core::types::{AppIdentity, WindowTitle};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows::Win32::System::ProcessStatus::QueryFullProcessImageNameW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
};
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
        let process_name = exe_path.as_ref().and_then(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
        });

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
        // QueryFullProcessImageNameW tends to be more reliable with limited permissions.
        // Fall back to K32GetModuleFileNameExW if needed.
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .or_else(|_| OpenProcess(PROCESS_QUERY_INFORMATION, false, pid))?;

        // Start with a reasonably large buffer; retry if Windows reports it is too small.
        let mut buf = vec![0u16; 4096];
        let mut size: u32 = buf.len().try_into().unwrap_or(u32::MAX);

        let ok = QueryFullProcessImageNameW(Some(handle), 0, &mut buf, &mut size).is_ok();
        if ok && size > 0 {
            buf.truncate(size as usize);
            let _ = CloseHandle(handle);
            let os = OsString::from_wide(&buf);
            return Ok(os.to_string_lossy().to_string());
        }

        // Fallback: module file name.
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
