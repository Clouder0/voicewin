// Windows-specific implementations.
//
// We keep them behind `cfg(windows)` so the workspace builds cleanly on Linux.

use std::mem::size_of;
use std::time::Instant;

#[path = "windows_foreground.rs"]
mod windows_foreground;

use crate::screenshot::{bgra_to_rgba, encode_screenshot_data_url};
use voicewin_core::context::{ScreenshotCaptureMetadata, VisualCaptureScope};
use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{
    AppContextProvider, CapturedScreenshot, ContextSnapshot, Inserter, ScreenshotCaptureOptions,
};

use clipboard_win::get_clipboard_string;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, RECT, SM_CXSCREEN, SM_CYSCREEN,
};

#[path = "windows_insert.rs"]
mod windows_insert;

#[derive(Debug, Default)]
pub struct WindowsContextProvider;

#[async_trait::async_trait]
impl AppContextProvider for WindowsContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        windows_foreground::get_foreground_app_identity()
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        // MVP: provide window/app identity and clipboard text (best-effort).
        let app = self.foreground_app().await?;
        let mut ctx = ContextSnapshot::default();

        let proc = app
            .process_name
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_else(|| "unknown".into());
        let title = app
            .window_title
            .as_ref()
            .map(|t| t.0.clone())
            .unwrap_or_default();

        ctx.window_context = Some(format!("Application: {}\nActive Window: {}", proc, title));
        ctx.clipboard = get_clipboard_string().ok();
        ctx.selected_text = read_selected_text().ok().flatten();
        Ok(ctx)
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        capture_windows_screenshot(options)
    }
}

#[derive(Debug, Default)]
pub struct WindowsInserter;

#[async_trait::async_trait]
impl Inserter for WindowsInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        // MVP (reliable): clipboard swap + Ctrl+V + optional Enter + restore.
        windows_insert::paste_text_via_clipboard(text, mode)
    }
}

const WM_GETTEXT: u32 = 0x000D;
const WM_GETTEXTLENGTH: u32 = 0x000E;
const EM_GETSEL: u32 = 0x00B0;

fn read_selected_text() -> anyhow::Result<Option<String>> {
    unsafe {
        let mut gui = windows::Win32::UI::WindowsAndMessaging::GUITHREADINFO {
            cbSize: std::mem::size_of::<windows::Win32::UI::WindowsAndMessaging::GUITHREADINFO>()
                as u32,
            ..Default::default()
        };

        if !windows::Win32::UI::WindowsAndMessaging::GetGUIThreadInfo(0, &mut gui).as_bool() {
            return Ok(None);
        }

        let hwnd = if !gui.hwndFocus.0.is_null() {
            gui.hwndFocus
        } else {
            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()
        };
        if hwnd.0.is_null() || !is_probably_text_selection_host(hwnd)? {
            return Ok(None);
        }

        let mut selection_start = 0u32;
        let mut selection_end = 0u32;
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_GETSEL,
            WPARAM((&mut selection_start as *mut u32) as usize),
            LPARAM((&mut selection_end as *mut u32) as isize),
        );
        if selection_end <= selection_start {
            return Ok(None);
        }

        let text_len = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_GETTEXTLENGTH,
            WPARAM(0),
            LPARAM(0),
        )
        .0 as usize;
        if text_len == 0 {
            return Ok(None);
        }

        let mut text = vec![0u16; text_len + 1];
        let copied = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_GETTEXT,
            WPARAM(text.len()),
            LPARAM(text.as_mut_ptr() as isize),
        )
        .0 as usize;
        if copied == 0 {
            return Ok(None);
        }
        text.truncate(copied);

        let start = selection_start as usize;
        let end = (selection_end as usize).min(text.len());
        if start >= end || start >= text.len() {
            return Ok(None);
        }

        let selected = String::from_utf16_lossy(&text[start..end]);
        Ok((!selected.trim().is_empty()).then_some(selected))
    }
}

fn is_probably_text_selection_host(hwnd: HWND) -> anyhow::Result<bool> {
    let class_name = get_class_name(hwnd)?;
    let class_name = class_name.trim().to_ascii_lowercase();
    Ok(class_name == "edit" || class_name.contains("richedit") || class_name.contains("scintilla"))
}

fn get_class_name(hwnd: HWND) -> anyhow::Result<String> {
    unsafe {
        let mut buf = vec![0u16; 256];
        let copied = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf);
        if copied == 0 {
            return Err(anyhow::anyhow!(
                "failed to query focused control class name"
            ));
        }
        buf.truncate(copied as usize);
        Ok(String::from_utf16_lossy(&buf))
    }
}

fn capture_windows_screenshot(
    options: ScreenshotCaptureOptions,
) -> anyhow::Result<Option<CapturedScreenshot>> {
    match options.scope {
        VisualCaptureScope::Display => capture_screen_region_screenshot(
            0,
            0,
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
            options,
            "display",
            None,
        ),
        VisualCaptureScope::ForegroundWindow => capture_foreground_window_screenshot(options),
    }
}

fn capture_foreground_window_screenshot(
    options: ScreenshotCaptureOptions,
) -> anyhow::Result<Option<CapturedScreenshot>> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            log::debug!(
                "windows screenshot capture: requested_scope=foreground_window fallback_scope=display reason=no_foreground_window"
            );
            return capture_screen_region_screenshot(
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                options,
                "display_fallback",
                Some("no_foreground_window"),
            );
        }

        let mut rect = RECT::default();
        if !GetWindowRect(hwnd, &mut rect).as_bool() {
            log::debug!(
                "windows screenshot capture: requested_scope=foreground_window fallback_scope=display reason=get_window_rect_failed"
            );
            return capture_screen_region_screenshot(
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                options,
                "display_fallback",
                Some("get_window_rect_failed"),
            );
        }

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let left = rect.left.clamp(0, screen_width);
        let top = rect.top.clamp(0, screen_height);
        let right = rect.right.clamp(0, screen_width);
        let bottom = rect.bottom.clamp(0, screen_height);
        let width = right - left;
        let height = bottom - top;
        if width <= 0 || height <= 0 {
            log::debug!(
                "windows screenshot capture: requested_scope=foreground_window fallback_scope=display reason=empty_window_rect left={} top={} right={} bottom={}",
                rect.left,
                rect.top,
                rect.right,
                rect.bottom
            );
            return capture_screen_region_screenshot(
                0,
                0,
                screen_width,
                screen_height,
                options,
                "display_fallback",
                Some("empty_window_rect"),
            );
        }

        capture_screen_region_screenshot(
            left,
            top,
            width,
            height,
            options,
            "foreground_window",
            None,
        )
    }
}

fn capture_screen_region_screenshot(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    options: ScreenshotCaptureOptions,
    scope_label: &str,
    fallback_reason: Option<&str>,
) -> anyhow::Result<Option<CapturedScreenshot>> {
    let started = Instant::now();

    unsafe {
        if width <= 0 || height <= 0 {
            return Ok(None);
        }

        let screen_dc = GetDC(None);
        anyhow::ensure!(
            !screen_dc.is_invalid(),
            "GetDC returned an invalid screen DC"
        );
        let screen_dc_guard = ScreenDc(screen_dc);

        let memory_dc = CreateCompatibleDC(Some(screen_dc_guard.0));
        anyhow::ensure!(
            !memory_dc.is_invalid(),
            "CreateCompatibleDC returned an invalid memory DC"
        );
        let memory_dc_guard = MemoryDc(memory_dc);

        let bitmap = CreateCompatibleBitmap(screen_dc_guard.0, width, height);
        anyhow::ensure!(
            !bitmap.is_invalid(),
            "CreateCompatibleBitmap returned an invalid bitmap"
        );
        let bitmap_guard = Bitmap(bitmap);

        let previous = SelectObject(memory_dc_guard.0, HGDIOBJ(bitmap_guard.0.0));
        anyhow::ensure!(
            !previous.is_invalid(),
            "SelectObject failed to select the screenshot bitmap"
        );
        let selected_bitmap_guard = SelectedBitmap {
            dc: memory_dc_guard.0,
            previous,
        };

        BitBlt(
            memory_dc_guard.0,
            0,
            0,
            width,
            height,
            Some(screen_dc_guard.0),
            left,
            top,
            SRCCOPY,
        )?;

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        let copied = GetDIBits(
            memory_dc_guard.0,
            bitmap_guard.0,
            0,
            height as u32,
            Some(bgra.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );
        anyhow::ensure!(
            copied == height,
            "GetDIBits copied {copied} scanlines, expected {height}"
        );

        drop(selected_bitmap_guard);

        let artifact = encode_screenshot_data_url(
            width as u32,
            height as u32,
            bgra_to_rgba(&bgra)?,
            options.max_edge_px,
        )?;
        let elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        log::debug!(
            "captured Windows screenshot: scope={} left={} top={} width={} height={} elapsed_ms={} data_url_bytes={} max_edge_px={}",
            scope_label,
            left,
            top,
            width,
            height,
            elapsed_ms,
            artifact.data_url.len(),
            options.max_edge_px
        );
        Ok(Some(CapturedScreenshot {
            image: artifact,
            metadata: ScreenshotCaptureMetadata {
                actual_scope: Some(match scope_label {
                    "foreground_window" => VisualCaptureScope::ForegroundWindow,
                    _ => VisualCaptureScope::Display,
                }),
                capture_elapsed_ms: Some(elapsed_ms),
                fallback_reason: fallback_reason.map(str::to_string),
            },
        }))
    }
}

struct ScreenDc(HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseDC(None, self.0);
        }
    }
}

struct MemoryDc(HDC);

impl Drop for MemoryDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

struct SelectedBitmap {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Drop for SelectedBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
        }
    }
}
