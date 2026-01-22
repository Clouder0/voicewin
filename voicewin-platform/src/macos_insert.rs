// macOS clipboard-based paste.
//
// Requirements (MVP):
// - Preserve the full NSPasteboard contents (all items, all types/data) and restore after paste.
// - Paste using CGEvent Cmd+V (no AppleScript fallback).
// - Requires Accessibility permission (AXIsProcessTrusted).
//
// This file is only compiled on macOS.

#![cfg(target_os = "macos")]

use std::thread;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, runtime::ProtocolObject};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardType, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString};

use voicewin_core::types::InsertMode;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const AnyObject) -> bool;
    static kAXTrustedCheckOptionPrompt: *const AnyObject;
}

fn is_accessibility_trusted() -> bool {
    // Mirror enigo's approach: AXIsProcessTrustedWithOptions({ prompt: false }).
    unsafe {
        let key = CFString::wrap_under_create_rule(kAXTrustedCheckOptionPrompt.cast());
        let value = core_foundation::boolean::CFBoolean::false_value();
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef().cast())
    }
}

#[derive(Debug, Clone)]
struct PasteboardItemSnapshot {
    // Vec of (UTI/type string, raw bytes)
    types: Vec<(String, Vec<u8>)>,
}

const SNAPSHOT_MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

fn snapshot_pasteboard(pasteboard: &NSPasteboard) -> Vec<PasteboardItemSnapshot> {
    let mut out = Vec::new();
    let mut total = 0usize;

    // `pasteboardItems` may be nil.
    let items: Option<Retained<NSArray<NSPasteboardItem>>> = unsafe { pasteboard.pasteboardItems() };
    let Some(items) = items else {
        return out;
    };

    for item in items.iter() {
        let mut entry = PasteboardItemSnapshot { types: Vec::new() };

        // Each item has a list of types.
        let types: Retained<NSArray<NSPasteboardType>> = unsafe { item.types() };
        for t in types.iter() {
            // Convert NSPasteboardType (NSString newtype) to Rust string.
            let ty: Retained<NSString> = unsafe { msg_send![t, copy] };
            let ty_str = ty.to_string();

            // Fetch raw data for this type.
            let data: Option<Retained<NSData>> = unsafe { item.dataForType(&*t) };
            let Some(data) = data else {
                continue;
            };

            let bytes = data.bytes();
            let len = data.length();
            if len == 0 {
                continue;
            }

            if total.saturating_add(len) > SNAPSHOT_MAX_TOTAL_BYTES {
                // Too large; don't attempt "full" restoration.
                return Vec::new();
            }

            // SAFETY: NSData guarantees `bytes` is valid for `length` bytes.
            let slice = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) };
            entry.types.push((ty_str, slice.to_vec()));
            total += len;
        }

        if !entry.types.is_empty() {
            out.push(entry);
        }
    }

    out
}

fn restore_pasteboard(pasteboard: &NSPasteboard, snapshot: &[PasteboardItemSnapshot]) {
    unsafe {
        pasteboard.clearContents();
    }

    if snapshot.is_empty() {
        return;
    }

    // Recreate items and write them back.
    let mut items: Vec<Retained<NSPasteboardItem>> = Vec::with_capacity(snapshot.len());

    for item in snapshot {
        let pb_item = unsafe { NSPasteboardItem::new() };

        for (ty, bytes) in &item.types {
            // NSPasteboardType is a typedef of NSString.
            let ns_ty = NSString::from_str(ty);
            let ns_data = NSData::with_bytes(bytes);
            let _ok: bool = unsafe { pb_item.setData_forType(&ns_data, &ns_ty) };
        }

        items.push(pb_item);
    }

    // Write all items back.
    // NSPasteboard expects NSArray<id<NSPasteboardWriting>>.
    let mut as_proto: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        Vec::with_capacity(items.len());

    for it in items {
        as_proto.push(ProtocolObject::from_retained(it));
    }

    let objects = NSArray::from_retained_slice(&as_proto);
    let _ = pasteboard.writeObjects(&objects);
}

fn post_cmd_v() -> anyhow::Result<()> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("failed to create CGEventSource"))?;

    // Keycodes: Command = 0x37, V = 0x09, Return = 0x24 (VoiceInk uses same).
    let cmd_key: u16 = 0x37;
    let v_key: u16 = 0x09;

    let mut flags = CGEventFlags::CGEventFlagCommand;

    // Cmd down
    let cmd_down = CGEvent::new_keyboard_event(src.clone(), cmd_key, true)
        .ok_or_else(|| anyhow::anyhow!("failed to create cmd down event"))?;
    cmd_down.set_flags(flags);
    cmd_down.post(CGEventTapLocation::HID);

    // V down
    let v_down = CGEvent::new_keyboard_event(src.clone(), v_key, true)
        .ok_or_else(|| anyhow::anyhow!("failed to create v down event"))?;
    v_down.set_flags(flags);
    v_down.post(CGEventTapLocation::HID);

    // V up
    let v_up = CGEvent::new_keyboard_event(src.clone(), v_key, false)
        .ok_or_else(|| anyhow::anyhow!("failed to create v up event"))?;
    v_up.set_flags(flags);
    v_up.post(CGEventTapLocation::HID);

    // Cmd up (no flags)
    flags.remove(CGEventFlags::CGEventFlagCommand);
    let cmd_up = CGEvent::new_keyboard_event(src, cmd_key, false)
        .ok_or_else(|| anyhow::anyhow!("failed to create cmd up event"))?;
    cmd_up.set_flags(flags);
    cmd_up.post(CGEventTapLocation::HID);

    Ok(())
}

fn post_enter() -> anyhow::Result<()> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("failed to create CGEventSource"))?;

    let enter_key: u16 = 0x24;

    let down = CGEvent::new_keyboard_event(src.clone(), enter_key, true)
        .ok_or_else(|| anyhow::anyhow!("failed to create enter down event"))?;
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(src, enter_key, false)
        .ok_or_else(|| anyhow::anyhow!("failed to create enter up event"))?;
    up.post(CGEventTapLocation::HID);

    Ok(())
}

pub fn paste_text_via_clipboard(text: &str, mode: InsertMode) -> anyhow::Result<()> {
    if !is_accessibility_trusted() {
        return Err(anyhow::anyhow!(
            "Accessibility permission is required to paste into other apps (enable it in System Settings → Privacy & Security → Accessibility)."
        ));
    }

    let pasteboard = NSPasteboard::generalPasteboard();

    let original_change = pasteboard.changeCount();

    // Snapshot full pasteboard.
    let snapshot = snapshot_pasteboard(&pasteboard);

    // Write our text.
    unsafe {
        pasteboard.clearContents();
    }

    let ns_text = NSString::from_str(text);
    let _ = pasteboard.setString_forType(&ns_text, NSPasteboardTypeString);
    let after_write_change = pasteboard.changeCount();

    // Small delay to ensure the target app sees clipboard update.
    thread::sleep(Duration::from_millis(50));

    post_cmd_v()?;

    if matches!(mode, InsertMode::PasteAndEnter) {
        thread::sleep(Duration::from_millis(50));
        post_enter()?;
    }

    // Restore pasteboard after a delay, but only if the user/app hasn't changed it.
    thread::sleep(Duration::from_millis(1000));

    let current_change = pasteboard.changeCount();
    if current_change == after_write_change || current_change == original_change {
        restore_pasteboard(&pasteboard, &snapshot);
    }

    Ok(())
}
