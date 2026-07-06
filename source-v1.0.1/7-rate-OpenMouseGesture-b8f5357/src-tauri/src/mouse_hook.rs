use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use windows::{
    core::*, Win32::Foundation::*, Win32::System::Threading::*, Win32::UI::WindowsAndMessaging::*,
};

const LLMHF_INJECTED: u32 = 0x00000001;
const LLKHF_INJECTED: u32 = 0x00000010;
const SMALL_MOVE_POINTS: usize = 8;
const PREVIEW_MIN_POINTS: usize = 6;
const PREVIEW_INTERVAL_MS: u64 = 16;

static MOUSE_HOOK_HANDLE: Mutex<Option<isize>> = Mutex::new(None);
static KEYBOARD_HOOK_HANDLE: Mutex<Option<isize>> = Mutex::new(None);
static TRAJECTORY: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
static IS_DRAGGING: Mutex<bool> = Mutex::new(false);
static IS_LEFT_PRESSED: Mutex<bool> = Mutex::new(false);
static GESTURE_START_WINDOW: Mutex<Option<isize>> = Mutex::new(None);
static ACTIVE_TEMPLATES: Mutex<Vec<crate::config::GestureTemplate>> = Mutex::new(Vec::new());
static ACTIVE_CONFIG: Mutex<Option<crate::config::Config>> = Mutex::new(None);
static ACTIVE_TRIGGER_SLOT: Mutex<Option<String>> = Mutex::new(None);
static LAST_PREVIEW_AT: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_PREVIEW_KEY: Mutex<Option<String>> = Mutex::new(None);
static PRESSED_KEYS: LazyLock<Mutex<HashSet<u16>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn parse_mouse_trigger(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "mouse:left" => Some("left"),
        "right" | "mouse:right" => Some("right"),
        "middle" | "mouse:middle" => Some("middle"),
        "x1" | "mouse:x1" => Some("x1"),
        "x2" | "mouse:x2" => Some("x2"),
        _ => None,
    }
}

fn get_window_exe_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        if process_id == 0 {
            return None;
        }

        let process_handle =
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        let mut exe_path = [0u16; 260];
        let mut size = exe_path.len() as u32;

        if QueryFullProcessImageNameW(
            process_handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(exe_path.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
        {
            let _ = CloseHandle(process_handle);
            let path_str = String::from_utf16_lossy(&exe_path[..size as usize]);
            std::path::Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
        } else {
            let _ = CloseHandle(process_handle);
            None
        }
    }
}

fn is_ignored_by_global_config(exe_name: &str) -> bool {
    if let Ok(manager) = crate::config::ConfigManager::new() {
        if let Ok(config) = manager.load_config() {
            return config
                .ignore_exe
                .iter()
                .any(|e| e.to_lowercase() == exe_name);
        }
    }
    false
}

fn clear_preview_state() {
    *LAST_PREVIEW_KEY.lock().unwrap() = None;
    *LAST_PREVIEW_AT.lock().unwrap() = None;
    crate::clear_action_label_overlay();
}

fn action_preview_key(action: &crate::config::Action) -> String {
    crate::action_key_for_action(action)
}

fn should_run_preview() -> bool {
    let mut last_preview_at = LAST_PREVIEW_AT.lock().unwrap();
    let now = Instant::now();
    let should_run = match *last_preview_at {
        Some(last) => now.duration_since(last) >= Duration::from_millis(PREVIEW_INTERVAL_MS),
        None => true,
    };

    if should_run {
        *last_preview_at = Some(now);
    }

    should_run
}

fn update_recognition_preview(force: bool) {
    if !force && !should_run_preview() {
        return;
    }

    let points_snapshot = TRAJECTORY.lock().unwrap().clone();
    let active_slot = ACTIVE_TRIGGER_SLOT.lock().unwrap().clone();

    if points_snapshot.len() < PREVIEW_MIN_POINTS || active_slot.is_none() {
        if LAST_PREVIEW_KEY.lock().unwrap().is_some() {
            clear_preview_state();
        }
        return;
    }

    let templates = ACTIVE_TEMPLATES.lock().unwrap().clone();
    let config = ACTIVE_CONFIG.lock().unwrap().clone();

    let Some(config) = config else {
        clear_preview_state();
        return;
    };
    let slot = active_slot.unwrap();

    let points: Vec<(f64, f64)> = points_snapshot
        .iter()
        .map(|(x, y)| (*x as f64, *y as f64))
        .collect();

    let Some(gesture_name) = crate::gesture_recognizer::recognize(&points, &templates) else {
        if LAST_PREVIEW_KEY.lock().unwrap().is_some() {
            clear_preview_state();
        }
        return;
    };

    let Some(action) = crate::find_action_for_gesture(&config, &slot, &gesture_name) else {
        if LAST_PREVIEW_KEY.lock().unwrap().is_some() {
            clear_preview_state();
        }
        return;
    };

    let preview_key = action_preview_key(action);
    *LAST_PREVIEW_KEY.lock().unwrap() = Some(preview_key);
    crate::show_action_label_for_action(action);
}

fn load_active_resources() {
    if let Ok(manager) = crate::config::ConfigManager::new() {
        if let Ok(config) = manager.load_config() {
            *ACTIVE_CONFIG.lock().unwrap() = Some(config);
        }
        if let Ok(templates) = manager.load_gestures() {
            *ACTIVE_TEMPLATES.lock().unwrap() = templates;
        }
    }
}

fn xbutton_name(mouse_data: &MSLLHOOKSTRUCT) -> Option<&'static str> {
    let x_button = (mouse_data.mouseData >> 16) & 0xFFFF;
    match x_button {
        1 => Some("x1"),
        2 => Some("x2"),
        _ => None,
    }
}

fn modifier_pressed(keys: &HashSet<u16>, modifier: &str) -> bool {
    match modifier {
        "Shift" => [0x10, 0xA0, 0xA1].iter().any(|code| keys.contains(code)),
        "Ctrl" => [0x11, 0xA2, 0xA3].iter().any(|code| keys.contains(code)),
        "Alt" => [0x12, 0xA4, 0xA5].iter().any(|code| keys.contains(code)),
        _ => false,
    }
}

fn keyboard_trigger_active(trigger: &str, keys: &HashSet<u16>) -> bool {
    let Some((modifiers, code)) = crate::config::parse_keyboard_trigger(trigger) else {
        return false;
    };
    let Some(vk_code) = crate::config::keyboard_code_to_vk(&code) else {
        return false;
    };
    keys.contains(&vk_code) && modifiers.iter().all(|modifier| modifier_pressed(keys, modifier))
}

fn keyboard_trigger_starts_on_vk(trigger: &str, vk_code: u16, keys: &HashSet<u16>) -> bool {
    let Some((modifiers, code)) = crate::config::parse_keyboard_trigger(trigger) else {
        return false;
    };
    let Some(trigger_vk) = crate::config::keyboard_code_to_vk(&code) else {
        return false;
    };
    trigger_vk == vk_code && modifiers.iter().all(|modifier| modifier_pressed(keys, modifier))
}

fn trigger_slot_for_mouse_down(
    config: &crate::config::Config,
    event_type: u32,
    mouse_data: &MSLLHOOKSTRUCT,
) -> Option<&'static str> {
    for slot in ["A", "B", "C"] {
        let trigger = crate::trigger_button_for_slot(config, slot);
        let Some(button) = parse_mouse_trigger(trigger) else {
            continue;
        };
        let matched = match button {
            "left" => event_type == WM_LBUTTONDOWN,
            "right" => event_type == WM_RBUTTONDOWN,
            "middle" => event_type == WM_MBUTTONDOWN,
            "x1" => event_type == WM_XBUTTONDOWN && xbutton_name(mouse_data) == Some("x1"),
            "x2" => event_type == WM_XBUTTONDOWN && xbutton_name(mouse_data) == Some("x2"),
            _ => false,
        };

        if matched {
            return Some(slot);
        }
    }

    None
}

fn active_mouse_trigger_matches_up(
    config: &crate::config::Config,
    slot: &str,
    event_type: u32,
    mouse_data: &MSLLHOOKSTRUCT,
) -> bool {
    let button = parse_mouse_trigger(crate::trigger_button_for_slot(config, slot));
    match button {
        Some("left") => event_type == WM_LBUTTONUP,
        Some("right") => event_type == WM_RBUTTONUP,
        Some("middle") => event_type == WM_MBUTTONUP,
        Some("x1") => event_type == WM_XBUTTONUP && xbutton_name(mouse_data) == Some("x1"),
        Some("x2") => event_type == WM_XBUTTONUP && xbutton_name(mouse_data) == Some("x2"),
        _ => false,
    }
}

fn trigger_slot_for_keyboard_down(
    config: &crate::config::Config,
    vk_code: u16,
    keys: &HashSet<u16>,
) -> Option<&'static str> {
    for slot in ["A", "B", "C"] {
        if keyboard_trigger_starts_on_vk(crate::trigger_button_for_slot(config, slot), vk_code, keys) {
            return Some(slot);
        }
    }
    None
}

fn current_cursor_point() -> POINT {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_ok() {
            point
        } else {
            POINT { x: 0, y: 0 }
        }
    }
}

fn resolve_window_for_point(point: POINT) -> HWND {
    unsafe {
        let window_at_point = WindowFromPoint(point);
        if window_at_point != HWND::default() {
            window_at_point
        } else {
            GetForegroundWindow()
        }
    }
}

fn begin_gesture(config: &crate::config::Config, slot: &str, point: POINT, current_window: HWND) {
    *GESTURE_START_WINDOW.lock().unwrap() = Some(current_window.0 as isize);
    *IS_DRAGGING.lock().unwrap() = true;
    *ACTIVE_TRIGGER_SLOT.lock().unwrap() = Some(slot.to_string());

    {
        let mut trajectory = TRAJECTORY.lock().unwrap();
        trajectory.clear();
        trajectory.push((point.x, point.y));
    }

    crate::set_active_trail_color(crate::color_for_trigger_slot(config, slot));
    clear_preview_state();
    crate::emit_trajectory_update(&[(point.x, point.y)], true);
}

fn complete_gesture(config: &crate::config::Config, slot: &str) {
    *IS_DRAGGING.lock().unwrap() = false;
    crate::emit_trajectory_update(&[], false);

    let points_snapshot = TRAJECTORY.lock().unwrap().clone();
    if !points_snapshot.is_empty() && crate::is_gesture_enabled_internal() {
        update_recognition_preview(true);
        let points: Vec<(f64, f64)> = points_snapshot
            .iter()
            .map(|(x, y)| (*x as f64, *y as f64))
            .collect();
        let templates = ACTIVE_TEMPLATES.lock().unwrap().clone();

        if let Some(gesture_name) = crate::gesture_recognizer::recognize(&points, &templates) {
            if let Some(action) = crate::find_action_for_gesture(config, slot, &gesture_name) {
                let target_hwnd = GESTURE_START_WINDOW
                    .lock()
                    .unwrap()
                    .map(|h| HWND(h as *mut _));

                if let Some(hwnd) = target_hwnd {
                    if let Some(exe_name) = get_window_exe_name(hwnd) {
                        if let Some(ref ignore_list) = action.ignore_exe {
                            if ignore_list.iter().any(|e| e.to_lowercase() == exe_name) {
                                clear_preview_state();
                                TRAJECTORY.lock().unwrap().clear();
                                *ACTIVE_TRIGGER_SLOT.lock().unwrap() = None;
                                *GESTURE_START_WINDOW.lock().unwrap() = None;
                                return;
                            }
                        }
                    }
                }

                let _ = crate::command_executor::execute_action_with_window(action, target_hwnd, true);
                crate::emit_gesture_recognized(&gesture_name, Some(&action.action_type));
            }
        } else if points.len() <= SMALL_MOVE_POINTS {
            if slot == "A" && parse_mouse_trigger(crate::trigger_button_for_slot(config, slot)) == Some("right") {
                let mouse_pos = points[0];
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    crate::command_executor::send_right_click(mouse_pos.0 as i32, mouse_pos.1 as i32);
                });
            }
        }
    }

    clear_preview_state();
    TRAJECTORY.lock().unwrap().clear();
    *ACTIVE_TRIGGER_SLOT.lock().unwrap() = None;
    *GESTURE_START_WINDOW.lock().unwrap() = None;
}

unsafe extern "system" fn mouse_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let mouse_data = *(l_param.0 as *const MSLLHOOKSTRUCT);
    if (mouse_data.flags & LLMHF_INJECTED) != 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let event_type = w_param.0 as u32;

    match event_type {
        WM_LBUTTONDOWN => {
            *IS_LEFT_PRESSED.lock().unwrap() = true;
        }
        WM_LBUTTONUP => {
            *IS_LEFT_PRESSED.lock().unwrap() = false;
        }
        _ => {}
    }

    match event_type {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
            load_active_resources();
            let config = ACTIVE_CONFIG.lock().unwrap().clone();

            if let Some(config) = config {
                if let Some(slot) = trigger_slot_for_mouse_down(&config, event_type, &mouse_data) {
                    let point = POINT { x: mouse_data.pt.x, y: mouse_data.pt.y };
                    let current_window = resolve_window_for_point(point);

                    if current_window != HWND::default() {
                        if let Some(exe_name) = get_window_exe_name(current_window) {
                            if is_ignored_by_global_config(&exe_name) {
                                return CallNextHookEx(None, n_code, w_param, l_param);
                            }
                        }
                    }

                    begin_gesture(&config, slot, point, current_window);
                    return LRESULT(1);
                }
            }
        }
        WM_MOUSEMOVE => {
            if *IS_DRAGGING.lock().unwrap() {
                TRAJECTORY.lock().unwrap().push((mouse_data.pt.x, mouse_data.pt.y));
                crate::append_trajectory_point(mouse_data.pt.x, mouse_data.pt.y);
                update_recognition_preview(false);
            }
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
            if *IS_DRAGGING.lock().unwrap() {
                let config = ACTIVE_CONFIG.lock().unwrap().clone();
                let active_slot = ACTIVE_TRIGGER_SLOT.lock().unwrap().clone();
                if let (Some(config), Some(slot)) = (config, active_slot) {
                    if active_mouse_trigger_matches_up(&config, &slot, event_type, &mouse_data) {
                        complete_gesture(&config, &slot);
                        return LRESULT(1);
                    }
                }
            }
        }
        WM_MOUSEWHEEL => {
            if *IS_DRAGGING.lock().unwrap() {
                let is_left_pressed = *IS_LEFT_PRESSED.lock().unwrap();
                let wheel_delta = ((mouse_data.mouseData >> 16) & 0xFFFF) as i16;
                let wheel_direction = if wheel_delta > 0 { "up" } else { "down" };
                let wheel_trigger = if is_left_pressed {
                    format!("leftclick_wheel_{}", wheel_direction)
                } else {
                    format!("wheel_{}", wheel_direction)
                };

                if let Some(config) = ACTIVE_CONFIG.lock().unwrap().clone() {
                    if let Some(action) = config.actions.iter().find(|a| {
                        a.trigger_type == "wheel"
                            && a.wheel_trigger.as_ref().map_or(false, |wt| wt == &wheel_trigger)
                    }) {
                        let target_hwnd = GESTURE_START_WINDOW.lock().unwrap().map(|h| HWND(h as *mut _));
                        let _ = crate::command_executor::execute_action_with_window(action, target_hwnd, false);
                        TRAJECTORY.lock().unwrap().clear();
                    }
                }

                return LRESULT(1);
            }
        }
        _ => {}
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let keyboard_data = *(l_param.0 as *const KBDLLHOOKSTRUCT);
    if (keyboard_data.flags.0 & LLKHF_INJECTED) != 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let event_type = w_param.0 as u32;
    let vk_code = keyboard_data.vkCode as u16;

    match event_type {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let pressed_snapshot = {
                let mut pressed_keys = PRESSED_KEYS.lock().unwrap();
                pressed_keys.insert(vk_code);
                pressed_keys.clone()
            };

            if !*IS_DRAGGING.lock().unwrap() {
                load_active_resources();
                let config = ACTIVE_CONFIG.lock().unwrap().clone();
                if let Some(config) = config {
                    if let Some(slot) = trigger_slot_for_keyboard_down(&config, vk_code, &pressed_snapshot) {
                        let point = current_cursor_point();
                        let current_window = resolve_window_for_point(point);

                        if current_window != HWND::default() {
                            if let Some(exe_name) = get_window_exe_name(current_window) {
                                if is_ignored_by_global_config(&exe_name) {
                                    return CallNextHookEx(None, n_code, w_param, l_param);
                                }
                            }
                        }

                        begin_gesture(&config, slot, point, current_window);
                    }
                }
            }
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let pressed_snapshot = {
                let mut pressed_keys = PRESSED_KEYS.lock().unwrap();
                pressed_keys.remove(&vk_code);
                pressed_keys.clone()
            };

            if *IS_DRAGGING.lock().unwrap() {
                let config = ACTIVE_CONFIG.lock().unwrap().clone();
                let active_slot = ACTIVE_TRIGGER_SLOT.lock().unwrap().clone();
                if let (Some(config), Some(slot)) = (config, active_slot) {
                    let trigger = crate::trigger_button_for_slot(&config, &slot);
                    if crate::config::parse_keyboard_trigger(trigger).is_some()
                        && !keyboard_trigger_active(trigger, &pressed_snapshot)
                    {
                        complete_gesture(&config, &slot);
                    }
                }
            }
        }
        _ => {}
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

pub fn install_hook() -> Result<()> {
    unsafe {
        if MOUSE_HOOK_HANDLE.lock().unwrap().is_none() {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0)?;
            *MOUSE_HOOK_HANDLE.lock().unwrap() = Some(hook.0 as isize);
        }

        if KEYBOARD_HOOK_HANDLE.lock().unwrap().is_none() {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)?;
            *KEYBOARD_HOOK_HANDLE.lock().unwrap() = Some(hook.0 as isize);
        }
    }
    Ok(())
}

pub fn uninstall_hook() -> Result<()> {
    unsafe {
        let mut mouse_handle = MOUSE_HOOK_HANDLE.lock().unwrap();
        if let Some(handle) = *mouse_handle {
            UnhookWindowsHookEx(HHOOK(handle as *mut _))?;
            *mouse_handle = None;
        }

        let mut keyboard_handle = KEYBOARD_HOOK_HANDLE.lock().unwrap();
        if let Some(handle) = *keyboard_handle {
            UnhookWindowsHookEx(HHOOK(handle as *mut _))?;
            *keyboard_handle = None;
        }
    }

    PRESSED_KEYS.lock().unwrap().clear();
    Ok(())
}
