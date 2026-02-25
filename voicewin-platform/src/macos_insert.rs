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
use std::{
    any::Any,
    ffi::c_void,
    mem::MaybeUninit,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
};

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardType, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString};

use voicewin_core::types::InsertMode;

#[repr(C)]
struct __TISInputSource;
type TISInputSourceRef = *const __TISInputSource;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const AnyObject) -> bool;
    static kAXTrustedCheckOptionPrompt: *const AnyObject;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCreateInputSourceList(
        properties: core_foundation::dictionary::CFDictionaryRef,
        includeAllInstalled: bool,
    ) -> core_foundation::array::CFArrayRef;
    fn TISGetInputSourceProperty(
        inputSource: TISInputSourceRef,
        propertyKey: CFStringRef,
    ) -> *const AnyObject;
    fn TISSelectInputSource(inputSource: TISInputSourceRef) -> core_foundation::base::OSStatus;

    static kTISPropertyInputSourceID: CFStringRef;
}

type DispatchQueueRef = *mut c_void;

#[link(name = "dispatch")]
unsafe extern "C" {
    fn dispatch_get_main_queue() -> DispatchQueueRef;
    fn dispatch_sync_f(
        queue: DispatchQueueRef,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

#[link(name = "pthread")]
unsafe extern "C" {
    fn pthread_main_np() -> i32;
}

struct MainQueueSyncContext<F, R> {
    f: Option<F>,
    result: MaybeUninit<R>,
    panicked: Option<Box<dyn Any + Send + 'static>>,
}

extern "C" fn main_queue_sync_trampoline<F, R>(context: *mut c_void)
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    // SAFETY: The caller guarantees `context` is a valid pointer to
    // `MainQueueSyncContext<F, R>` for the duration of this call.
    let ctx = unsafe { &mut *(context as *mut MainQueueSyncContext<F, R>) };
    let Some(f) = ctx.f.take() else {
        return;
    };

    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => {
            ctx.result.write(v);
        }
        Err(panic) => {
            ctx.panicked = Some(panic);
        }
    }
}

fn run_on_main_queue_sync<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    // On macOS 15+, some HIToolbox/TSM APIs (e.g. TIS/TSM input source functions)
    // will hard-trap if called off the main dispatch queue.
    //
    // `dispatch_sync` to the main queue to guarantee correctness.
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }

    let mut ctx = MainQueueSyncContext {
        f: Some(f),
        result: MaybeUninit::uninit(),
        panicked: None,
    };

    unsafe {
        dispatch_sync_f(
            dispatch_get_main_queue(),
            (&mut ctx as *mut MainQueueSyncContext<F, R>).cast::<c_void>(),
            main_queue_sync_trampoline::<F, R>,
        );
    }

    if let Some(panic) = ctx.panicked {
        resume_unwind(panic);
    }

    // SAFETY: The trampoline always writes `result` if it did not panic.
    unsafe { ctx.result.assume_init() }
}

pub(super) fn is_accessibility_trusted() -> bool {
    // Mirror enigo's approach: AXIsProcessTrustedWithOptions({ prompt: false }).
    unsafe {
        let key = CFString::wrap_under_create_rule(kAXTrustedCheckOptionPrompt.cast());
        let value = core_foundation::boolean::CFBoolean::false_value();
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef().cast())
    }
}

pub(super) fn prompt_accessibility_permission() -> bool {
    // Trigger the macOS system prompt (best-effort). The return value reflects
    // the current trust state; it may remain false until the user enables it.
    unsafe {
        let key = CFString::wrap_under_create_rule(kAXTrustedCheckOptionPrompt.cast());
        let value = core_foundation::boolean::CFBoolean::true_value();
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef().cast())
    }
}

fn input_source_id(source: TISInputSourceRef) -> Option<String> {
    if source.is_null() {
        return None;
    }

    let raw = unsafe { TISGetInputSourceProperty(source, kTISPropertyInputSourceID) };
    if raw.is_null() {
        return None;
    }

    let s = unsafe { CFString::wrap_under_get_rule(raw.cast()) };
    Some(s.to_string())
}

fn is_us_qwerty_input_source_id(id: &str) -> bool {
    matches!(id, "com.apple.keylayout.ABC" | "com.apple.keylayout.US")
}

fn select_input_source_by_id_impl(id: &str) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kTISPropertyInputSourceID) };
    let value = CFString::new(id);
    let properties = CFDictionary::from_CFType_pairs(&[(key, value)]);

    let list = unsafe { TISCreateInputSourceList(properties.as_concrete_TypeRef(), false) };
    if list.is_null() {
        log::warn!("Failed to query input source list for id={id}");
        return false;
    }

    let sources: CFArray<*const c_void> = unsafe { TCFType::wrap_under_create_rule(list) };
    let mut source: Option<TISInputSourceRef> = None;
    for raw in sources.iter() {
        source = Some(*raw as TISInputSourceRef);
        break;
    }

    let Some(source) = source else {
        log::warn!("No input source found for id={id}");
        return false;
    };

    let status = unsafe { TISSelectInputSource(source) };
    if status == 0 {
        return true;
    }

    log::warn!("Failed to select input source id={id}: OSStatus={status}");
    false
}

fn select_input_source_by_id(id: &str) -> bool {
    run_on_main_queue_sync(|| select_input_source_by_id_impl(id))
}

struct KeyboardInputSourceGuard {
    previous_id: Option<String>,
}

impl Drop for KeyboardInputSourceGuard {
    fn drop(&mut self) {
        let Some(previous_id) = self.previous_id.take() else {
            return;
        };

        let ok = select_input_source_by_id(previous_id.as_str());
        if !ok {
            log::warn!("Failed to restore input source after paste: id={previous_id}");
        }
    }
}

fn maybe_switch_to_us_qwerty_layout() -> Option<KeyboardInputSourceGuard> {
    run_on_main_queue_sync(|| {
        let current = unsafe { TISCopyCurrentKeyboardInputSource() };
        if current.is_null() {
            return None;
        }

        let previous_id = input_source_id(current);
        unsafe {
            core_foundation::base::CFRelease(current.cast());
        }

        let Some(previous_id) = previous_id else {
            return None;
        };

        if is_us_qwerty_input_source_id(previous_id.as_str()) {
            return None;
        }

        let mut switched_to: Option<&'static str> = None;
        if select_input_source_by_id_impl("com.apple.keylayout.ABC") {
            switched_to = Some("com.apple.keylayout.ABC");
        } else if select_input_source_by_id_impl("com.apple.keylayout.US") {
            switched_to = Some("com.apple.keylayout.US");
        }

        let Some(target_id) = switched_to else {
            log::warn!("Failed to switch keyboard layout for paste shortcut");
            return None;
        };

        log::info!("Switched keyboard layout to {target_id} for paste shortcut");
        Some(KeyboardInputSourceGuard {
            previous_id: Some(previous_id),
        })
    })
}

#[derive(Debug, Clone)]
struct PasteboardItemSnapshot {
    // Vec of (UTI/type string, raw bytes)
    types: Vec<(String, Vec<u8>)>,
}

const SNAPSHOT_MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

fn snapshot_pasteboard(pasteboard: &NSPasteboard) -> Option<Vec<PasteboardItemSnapshot>> {
    let mut out: Vec<PasteboardItemSnapshot> = Vec::new();
    let mut total = 0usize;

    // `pasteboardItems` may be nil.
    let items: Option<Retained<NSArray<NSPasteboardItem>>> = pasteboard.pasteboardItems();
    let Some(items) = items else {
        return Some(out);
    };

    for item in items.iter() {
        let mut entry = PasteboardItemSnapshot { types: Vec::new() };

        // Each item has a list of types.
        let types: Retained<NSArray<NSPasteboardType>> = item.types();
        for t in types.iter() {
            // NSPasteboardType is a typedef of NSString.
            let ty_str = t.to_string();

            // Fetch raw data for this type.
            let data: Option<Retained<NSData>> = item.dataForType(&t);
            let Some(data) = data else {
                continue;
            };

            let len = data.length() as usize;
            if len == 0 {
                continue;
            }

            if total.saturating_add(len) > SNAPSHOT_MAX_TOTAL_BYTES {
                // Too large; don't attempt "full" restoration.
                return None;
            }

            entry.types.push((ty_str, data.to_vec()));
            total += len;
        }

        if !entry.types.is_empty() {
            out.push(entry);
        }
    }

    Some(out)
}

fn restore_pasteboard(pasteboard: &NSPasteboard, snapshot: &[PasteboardItemSnapshot]) {
    pasteboard.clearContents();

    if snapshot.is_empty() {
        return;
    }

    // Recreate items and write them back.
    let mut items: Vec<Retained<NSPasteboardItem>> = Vec::with_capacity(snapshot.len());

    for item in snapshot {
        let pb_item = NSPasteboardItem::new();

        for (ty, bytes) in &item.types {
            // NSPasteboardType is a typedef of NSString.
            let ns_ty = NSString::from_str(ty);
            let ns_data = NSData::with_bytes(bytes);
            let _ok: bool = pb_item.setData_forType(&ns_data, &ns_ty);
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
        .map_err(|_| anyhow::anyhow!("failed to create cmd down event"))?;
    cmd_down.set_flags(flags);
    cmd_down.post(CGEventTapLocation::HID);

    // V down
    let v_down = CGEvent::new_keyboard_event(src.clone(), v_key, true)
        .map_err(|_| anyhow::anyhow!("failed to create v down event"))?;
    v_down.set_flags(flags);
    v_down.post(CGEventTapLocation::HID);

    // V up
    let v_up = CGEvent::new_keyboard_event(src.clone(), v_key, false)
        .map_err(|_| anyhow::anyhow!("failed to create v up event"))?;
    v_up.set_flags(flags);
    v_up.post(CGEventTapLocation::HID);

    // Cmd up (no flags)
    flags.remove(CGEventFlags::CGEventFlagCommand);
    let cmd_up = CGEvent::new_keyboard_event(src, cmd_key, false)
        .map_err(|_| anyhow::anyhow!("failed to create cmd up event"))?;
    cmd_up.set_flags(flags);
    cmd_up.post(CGEventTapLocation::HID);

    Ok(())
}

fn post_enter() -> anyhow::Result<()> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("failed to create CGEventSource"))?;

    let enter_key: u16 = 0x24;

    let down = CGEvent::new_keyboard_event(src.clone(), enter_key, true)
        .map_err(|_| anyhow::anyhow!("failed to create enter down event"))?;
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(src, enter_key, false)
        .map_err(|_| anyhow::anyhow!("failed to create enter up event"))?;
    up.post(CGEventTapLocation::HID);

    Ok(())
}

pub fn paste_text_via_clipboard(text: &str, mode: InsertMode) -> anyhow::Result<()> {
    let trusted = is_accessibility_trusted();

    let pasteboard = NSPasteboard::generalPasteboard();

    let original_change = pasteboard.changeCount();

    // Snapshot full pasteboard.
    let snapshot = if trusted {
        snapshot_pasteboard(&pasteboard)
    } else {
        None
    };

    // Write our text.
    pasteboard.clearContents();

    let ns_text = NSString::from_str(text);
    let text_type: &NSPasteboardType = unsafe { NSPasteboardTypeString };
    let _ = pasteboard.setString_forType(&ns_text, text_type);
    let after_write_change = pasteboard.changeCount();

    // Small delay to ensure the target app sees clipboard update.
    thread::sleep(Duration::from_millis(50));

    // Without Accessibility trust we cannot synthesize Cmd+V, but users should still
    // be able to paste manually.
    if !trusted {
        return Err(anyhow::anyhow!(
            "Accessibility permission is required to paste into other apps (enable it in System Settings → Privacy & Security → Accessibility). Copied to clipboard — press Cmd+V."
        ));
    }

    // Cmd+V uses a physical keycode. On non-US layouts, that key may not map to V,
    // so temporarily switch to an ASCII US layout before posting the shortcut.
    let _keyboard_layout_guard = maybe_switch_to_us_qwerty_layout();

    post_cmd_v()?;

    if matches!(mode, InsertMode::PasteAndEnter) {
        thread::sleep(Duration::from_millis(50));
        post_enter()?;
    }

    // macOS has no Shift+Insert paste convention; treat it like regular paste.
    // Nothing to do here since we already sent Cmd+V.

    // Restore pasteboard after a delay, but only if the user/app hasn't changed it.
    thread::sleep(Duration::from_millis(1000));

    let current_change = pasteboard.changeCount();
    if current_change == after_write_change || current_change == original_change {
        if let Some(snapshot) = snapshot.as_deref() {
            restore_pasteboard(&pasteboard, snapshot);
        } else {
            log::warn!("Skipping pasteboard restore: snapshot unavailable");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_us_qwerty_input_source_id;

    #[test]
    fn accepts_supported_us_layout_ids() {
        assert!(is_us_qwerty_input_source_id("com.apple.keylayout.ABC"));
        assert!(is_us_qwerty_input_source_id("com.apple.keylayout.US"));
    }

    #[test]
    fn rejects_non_us_layout_ids() {
        assert!(!is_us_qwerty_input_source_id("com.apple.keylayout.German"));
    }
}
