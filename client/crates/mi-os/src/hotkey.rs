//! 全局快捷键监听引擎 (`Alt+C` 复制广播 / `Alt+V` 智能粘贴) (FR-COPY-01, FR-PASTE-01).
//!
//! 跨平台原生实现（零额外第三方 crate 依赖）：
//! - macOS: Carbon `RegisterEventHotKey` + `InstallEventHandler`
//! - Windows: Win32 `user32.dll` `RegisterHotKey` + `GetMessageW`
//! - Linux: 动态加载 `libX11.so.6` (`XGrabKey` + `XNextEvent`)，纯 Wayland 环境优雅降级提示

use std::sync::mpsc::{self, Receiver, Sender};
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY_COPY: &str = "Alt+C";
pub const DEFAULT_HOTKEY_PASTE: &str = "Alt+V";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotkeyAction {
    BroadcastCopy,
    SmartPaste,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalHotkeyBinding {
    pub copy_chord: String,
    pub paste_chord: String,
}

impl Default for GlobalHotkeyBinding {
    fn default() -> Self {
        Self {
            copy_chord: DEFAULT_HOTKEY_COPY.to_string(),
            paste_chord: DEFAULT_HOTKEY_PASTE.to_string(),
        }
    }
}

impl GlobalHotkeyBinding {
    pub fn resolve_chord(&self, chord: &str) -> Option<HotkeyAction> {
        if chord.eq_ignore_ascii_case(&self.copy_chord) {
            Some(HotkeyAction::BroadcastCopy)
        } else if chord.eq_ignore_ascii_case(&self.paste_chord) {
            Some(HotkeyAction::SmartPaste)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedChord {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: char,
}

pub fn parse_hotkey_chord(chord: &str) -> Option<ParsedChord> {
    let mut alt = false;
    let mut ctrl = false;
    let mut shift = false;
    let mut super_key = false;
    let mut key: Option<char> = None;

    for part in chord.split('+').map(str::trim).filter(|s| !s.is_empty()) {
        if part.eq_ignore_ascii_case("alt") || part.eq_ignore_ascii_case("option") {
            alt = true;
        } else if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
            ctrl = true;
        } else if part.eq_ignore_ascii_case("shift") {
            shift = true;
        } else if part.eq_ignore_ascii_case("super")
            || part.eq_ignore_ascii_case("cmd")
            || part.eq_ignore_ascii_case("win")
        {
            super_key = true;
        } else if part.len() == 1 {
            let ch = part.chars().next()?.to_ascii_uppercase();
            if ch.is_ascii_alphabetic() {
                key = Some(ch);
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    Some(ParsedChord {
        alt,
        ctrl,
        shift,
        super_key,
        key: key?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyListenerStatus {
    pub active: bool,
    pub backend: String,
    pub note: String,
}

pub struct GlobalHotkeyHandle {
    pub status: HotkeyListenerStatus,
    #[cfg(target_os = "macos")]
    _inner: Option<macos_impl::MacosHotkeyGuard>,
    #[cfg(target_os = "windows")]
    _inner: Option<windows_impl::WindowsHotkeyGuard>,
    #[cfg(all(unix, not(target_os = "macos")))]
    _inner: Option<linux_impl::LinuxHotkeyGuard>,
}

impl GlobalHotkeyHandle {
    pub fn inactive(backend: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            status: HotkeyListenerStatus {
                active: false,
                backend: backend.into(),
                note: note.into(),
            },
            #[cfg(any(target_os = "macos", target_os = "windows", all(unix, not(target_os = "macos"))))]
            _inner: None,
        }
    }
}

/// 启动操作系统级全局热键监听 (`Alt+C` / `Alt+V`)，返回句柄与动作接收通道。
pub fn start_global_hotkey_listener(
    binding: &GlobalHotkeyBinding,
) -> anyhow::Result<(GlobalHotkeyHandle, Receiver<HotkeyAction>)> {
    let copy = parse_hotkey_chord(&binding.copy_chord)
        .ok_or_else(|| anyhow::anyhow!("Invalid copy hotkey chord: {}", binding.copy_chord))?;
    let paste = parse_hotkey_chord(&binding.paste_chord)
        .ok_or_else(|| anyhow::anyhow!("Invalid paste hotkey chord: {}", binding.paste_chord))?;

    let (tx, rx) = mpsc::channel();

    #[cfg(target_os = "macos")]
    {
        let guard = macos_impl::install(copy, paste, tx)?;
        let handle = GlobalHotkeyHandle {
            status: HotkeyListenerStatus {
                active: true,
                backend: "macOS Carbon RegisterEventHotKey".to_string(),
                note: format!("已注册全局热键 {} / {}", binding.copy_chord, binding.paste_chord),
            },
            _inner: Some(guard),
        };
        return Ok((handle, rx));
    }

    #[cfg(target_os = "windows")]
    {
        let guard = windows_impl::install(copy, paste, tx)?;
        let handle = GlobalHotkeyHandle {
            status: HotkeyListenerStatus {
                active: true,
                backend: "Windows RegisterHotKey".to_string(),
                note: format!("已注册全局热键 {} / {}", binding.copy_chord, binding.paste_chord),
            },
            _inner: Some(guard),
        };
        return Ok((handle, rx));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        match linux_impl::install(copy, paste, tx) {
            Ok(guard) => {
                let handle = GlobalHotkeyHandle {
                    status: HotkeyListenerStatus {
                        active: true,
                        backend: "Linux X11 XGrabKey".to_string(),
                        note: format!("已注册全局热键 {} / {}", binding.copy_chord, binding.paste_chord),
                    },
                    _inner: Some(guard),
                };
                return Ok((handle, rx));
            }
            Err(err) => {
                let handle = GlobalHotkeyHandle::inactive(
                    "Linux Wayland/Headless Fallback",
                    format!(
                        "未启用 X11 全局热键 ({}); Wayland 请在系统快捷键绑定 `mi-os-agent --trigger copy|paste`",
                        err
                    ),
                );
                return Ok((handle, rx));
            }
        }
    }
}

/// 在无 GUI 窗口的 `mi-os-agent` 进程中运行平台热键事件循环，直到收到退出信号。
pub fn run_agent_event_loop(stop_flag: &std::sync::atomic::AtomicBool) {
    #[cfg(target_os = "macos")]
    {
        macos_impl::run_loop_until_stopped(stop_flag);
    }
    #[cfg(not(target_os = "macos"))]
    {
        while !stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;
    use std::ffi::c_void;
    use std::mem;
    use std::ptr;

    const K_EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
    const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
    const K_EVENT_PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
    const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
    const HOTKEY_SIGNATURE: u32 = u32::from_be_bytes(*b"MINT");

    const CMD_KEY: u32 = 1 << 8;
    const SHIFT_KEY: u32 = 1 << 9;
    const OPTION_KEY: u32 = 1 << 11;
    const CONTROL_KEY: u32 = 1 << 12;

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[repr(C)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    type EventHandlerProc =
        extern "C" fn(next_handler: *mut c_void, the_event: *mut c_void, user_data: *mut c_void) -> i32;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetEventDispatcherTarget() -> *mut c_void;
        fn InstallEventHandler(
            in_target: *mut c_void,
            in_handler: EventHandlerProc,
            in_num_types: u32,
            in_list: *const EventTypeSpec,
            in_user_data: *mut c_void,
            out_ref: *mut *mut c_void,
        ) -> i32;
        fn RemoveEventHandler(in_handler_ref: *mut c_void) -> i32;
        fn RegisterEventHotKey(
            in_hot_key_code: u32,
            in_hot_key_modifiers: u32,
            in_hot_key_id: EventHotKeyID,
            in_target: *mut c_void,
            in_options: u32,
            out_ref: *mut *mut c_void,
        ) -> i32;
        fn UnregisterEventHotKey(in_hot_key: *mut c_void) -> i32;
        fn GetEventParameter(
            in_event: *mut c_void,
            in_name: u32,
            in_desired_type: u32,
            out_actual_type: *mut u32,
            in_buffer_size: u32,
            out_actual_size: *mut u32,
            out_data: *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopRunInMode(
            mode: *const c_void,
            seconds: f64,
            return_after_source_handled: u8,
        ) -> i32;
        static kCFRunLoopDefaultMode: *const c_void;
    }

    extern "C" fn hotkey_callback(
        _next_handler: *mut c_void,
        the_event: *mut c_void,
        user_data: *mut c_void,
    ) -> i32 {
        if user_data.is_null() {
            return -50;
        }
        let mut hk_id = EventHotKeyID {
            signature: 0,
            id: 0,
        };
        let status = unsafe {
            GetEventParameter(
                the_event,
                K_EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                ptr::null_mut(),
                mem::size_of::<EventHotKeyID>() as u32,
                ptr::null_mut(),
                (&mut hk_id as *mut EventHotKeyID).cast::<c_void>(),
            )
        };
        if status == 0 && hk_id.signature == HOTKEY_SIGNATURE {
            let tx = unsafe { &*(user_data as *const Sender<HotkeyAction>) };
            match hk_id.id {
                1 => {
                    let _ = tx.send(HotkeyAction::BroadcastCopy);
                }
                2 => {
                    let _ = tx.send(HotkeyAction::SmartPaste);
                }
                _ => {}
            }
            return 0;
        }
        -9874 // eventNotHandledErr
    }

    fn key_to_macos_vk(ch: char) -> Option<u32> {
        Some(match ch.to_ascii_uppercase() {
            'A' => 0,
            'S' => 1,
            'D' => 2,
            'F' => 3,
            'H' => 4,
            'G' => 5,
            'Z' => 6,
            'X' => 7,
            'C' => 8,
            'V' => 9,
            'B' => 11,
            'Q' => 12,
            'W' => 13,
            'E' => 14,
            'R' => 15,
            'Y' => 16,
            'T' => 17,
            'O' => 31,
            'U' => 32,
            'I' => 34,
            'P' => 35,
            'L' => 37,
            'J' => 38,
            'K' => 40,
            'N' => 45,
            'M' => 46,
            _ => return None,
        })
    }

    fn to_carbon_modifiers(chord: &ParsedChord) -> u32 {
        let mut m = 0u32;
        if chord.alt {
            m |= OPTION_KEY;
        }
        if chord.ctrl {
            m |= CONTROL_KEY;
        }
        if chord.shift {
            m |= SHIFT_KEY;
        }
        if chord.super_key {
            m |= CMD_KEY;
        }
        m
    }

    pub struct MacosHotkeyGuard {
        handler_ref: *mut c_void,
        copy_ref: *mut c_void,
        paste_ref: *mut c_void,
        tx_ptr: *mut Sender<HotkeyAction>,
    }

    unsafe impl Send for MacosHotkeyGuard {}

    impl Drop for MacosHotkeyGuard {
        fn drop(&mut self) {
            unsafe {
                if !self.copy_ref.is_null() {
                    let _ = UnregisterEventHotKey(self.copy_ref);
                }
                if !self.paste_ref.is_null() {
                    let _ = UnregisterEventHotKey(self.paste_ref);
                }
                if !self.handler_ref.is_null() {
                    let _ = RemoveEventHandler(self.handler_ref);
                }
                if !self.tx_ptr.is_null() {
                    drop(Box::from_raw(self.tx_ptr));
                }
            }
        }
    }

    pub fn install(
        copy: ParsedChord,
        paste: ParsedChord,
        tx: Sender<HotkeyAction>,
    ) -> anyhow::Result<MacosHotkeyGuard> {
        let copy_vk = key_to_macos_vk(copy.key)
            .ok_or_else(|| anyhow::anyhow!("Unsupported macOS key: {}", copy.key))?;
        let paste_vk = key_to_macos_vk(paste.key)
            .ok_or_else(|| anyhow::anyhow!("Unsupported macOS key: {}", paste.key))?;

        let tx_ptr = Box::into_raw(Box::new(tx));
        let event_type = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        };

        let mut handler_ref: *mut c_void = ptr::null_mut();
        let mut copy_ref: *mut c_void = ptr::null_mut();
        let mut paste_ref: *mut c_void = ptr::null_mut();

        unsafe {
            let target = GetEventDispatcherTarget();
            let st_handler = InstallEventHandler(
                target,
                hotkey_callback,
                1,
                &event_type,
                tx_ptr.cast::<c_void>(),
                &mut handler_ref,
            );
            if st_handler != 0 {
                drop(Box::from_raw(tx_ptr));
                anyhow::bail!("macOS InstallEventHandler failed (OSStatus {})", st_handler);
            }

            let st_copy = RegisterEventHotKey(
                copy_vk,
                to_carbon_modifiers(&copy),
                EventHotKeyID {
                    signature: HOTKEY_SIGNATURE,
                    id: 1,
                },
                target,
                0,
                &mut copy_ref,
            );
            if st_copy != 0 {
                let _ = RemoveEventHandler(handler_ref);
                drop(Box::from_raw(tx_ptr));
                anyhow::bail!("macOS RegisterEventHotKey(copy) failed (OSStatus {})", st_copy);
            }

            let st_paste = RegisterEventHotKey(
                paste_vk,
                to_carbon_modifiers(&paste),
                EventHotKeyID {
                    signature: HOTKEY_SIGNATURE,
                    id: 2,
                },
                target,
                0,
                &mut paste_ref,
            );
            if st_paste != 0 {
                let _ = UnregisterEventHotKey(copy_ref);
                let _ = RemoveEventHandler(handler_ref);
                drop(Box::from_raw(tx_ptr));
                anyhow::bail!("macOS RegisterEventHotKey(paste) failed (OSStatus {})", st_paste);
            }
        }

        Ok(MacosHotkeyGuard {
            handler_ref,
            copy_ref,
            paste_ref,
            tx_ptr,
        })
    }

    pub fn run_loop_until_stopped(stop_flag: &std::sync::atomic::AtomicBool) {
        while !stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            unsafe {
                let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    const WM_HOTKEY: u32 = 0x0312;
    const WM_QUIT: u32 = 0x0012;
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;
    const MOD_NOREPEAT: u32 = 0x4000;

    #[repr(C)]
    struct MSG {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterHotKey(h_wnd: *mut c_void, id: i32, fs_modifiers: u32, vk: u32) -> i32;
        fn UnregisterHotKey(h_wnd: *mut c_void, id: i32) -> i32;
        fn GetMessageW(lp_msg: *mut MSG, h_wnd: *mut c_void, min_f: u32, max_f: u32) -> i32;
        fn PostThreadMessageW(id_thread: u32, msg: u32, w_param: usize, l_param: isize) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    fn to_win_modifiers(chord: &ParsedChord) -> u32 {
        let mut m = MOD_NOREPEAT;
        if chord.alt {
            m |= MOD_ALT;
        }
        if chord.ctrl {
            m |= MOD_CONTROL;
        }
        if chord.shift {
            m |= MOD_SHIFT;
        }
        if chord.super_key {
            m |= MOD_WIN;
        }
        m
    }

    pub struct WindowsHotkeyGuard {
        thread_id: Arc<AtomicU32>,
        join: Option<JoinHandle<()>>,
    }

    impl Drop for WindowsHotkeyGuard {
        fn drop(&mut self) {
            let tid = self.thread_id.load(Ordering::SeqCst);
            if tid != 0 {
                unsafe {
                    let _ = PostThreadMessageW(tid, WM_QUIT, 0, 0);
                }
            }
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    pub fn install(
        copy: ParsedChord,
        paste: ParsedChord,
        tx: Sender<HotkeyAction>,
    ) -> anyhow::Result<WindowsHotkeyGuard> {
        let thread_id = Arc::new(AtomicU32::new(0));
        let tid_clone = thread_id.clone();
        let (init_tx, init_rx) = mpsc::channel::<anyhow::Result<()>>();

        let join = std::thread::spawn(move || unsafe {
            let tid = GetCurrentThreadId();
            tid_clone.store(tid, Ordering::SeqCst);

            let ok_copy = RegisterHotKey(
                ptr::null_mut(),
                1,
                to_win_modifiers(&copy),
                copy.key as u32,
            );
            if ok_copy == 0 {
                let _ = init_tx.send(Err(anyhow::anyhow!(
                    "Windows RegisterHotKey failed for copy chord"
                )));
                return;
            }
            let ok_paste = RegisterHotKey(
                ptr::null_mut(),
                2,
                to_win_modifiers(&paste),
                paste.key as u32,
            );
            if ok_paste == 0 {
                let _ = UnregisterHotKey(ptr::null_mut(), 1);
                let _ = init_tx.send(Err(anyhow::anyhow!(
                    "Windows RegisterHotKey failed for paste chord"
                )));
                return;
            }
            let _ = init_tx.send(Ok(()));

            let mut msg = MSG {
                hwnd: ptr::null_mut(),
                message: 0,
                w_param: 0,
                l_param: 0,
                time: 0,
                pt_x: 0,
                pt_y: 0,
            };
            while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    match msg.w_param {
                        1 => {
                            let _ = tx.send(HotkeyAction::BroadcastCopy);
                        }
                        2 => {
                            let _ = tx.send(HotkeyAction::SmartPaste);
                        }
                        _ => {}
                    }
                }
            }

            let _ = UnregisterHotKey(ptr::null_mut(), 1);
            let _ = UnregisterHotKey(ptr::null_mut(), 2);
        });

        init_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Windows hotkey thread terminated unexpectedly"))??;

        Ok(WindowsHotkeyGuard {
            thread_id,
            join: Some(join),
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux_impl {
    use super::*;
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::mem;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    const KEY_PRESS: c_int = 2;
    const GRAB_MODE_ASYNC: c_int = 1;
    const SHIFT_MASK: c_uint = 1 << 0;
    const LOCK_MASK: c_uint = 1 << 1; // CapsLock
    const CONTROL_MASK: c_uint = 1 << 2;
    const MOD1_MASK: c_uint = 1 << 3; // Alt
    const MOD2_MASK: c_uint = 1 << 4; // NumLock
    const MOD4_MASK: c_uint = 1 << 6; // Super

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct XKeyEvent {
        type_: c_int,
        serial: c_ulong,
        send_event: c_int,
        display: *mut c_void,
        window: c_ulong,
        root: c_ulong,
        subwindow: c_ulong,
        time: c_ulong,
        x: c_int,
        y: c_int,
        x_root: c_int,
        y_root: c_int,
        state: c_uint,
        keycode: c_uint,
        same_screen: c_int,
    }

    #[repr(C)]
    union XEvent {
        type_: c_int,
        xkey: XKeyEvent,
        pad: [c_long; 24],
    }

    type FnXOpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type FnXCloseDisplay = unsafe extern "C" fn(*mut c_void) -> c_int;
    type FnXDefaultRootWindow = unsafe extern "C" fn(*mut c_void) -> c_ulong;
    type FnXKeysymToKeycode = unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int;
    type FnXGrabKey =
        unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong, c_int, c_int, c_int) -> c_int;
    type FnXUngrabKey = unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong) -> c_int;
    type FnXPending = unsafe extern "C" fn(*mut c_void) -> c_int;
    type FnXNextEvent = unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int;
    type FnXSync = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

    fn to_x11_mask(chord: &ParsedChord) -> c_uint {
        let mut m = 0;
        if chord.alt {
            m |= MOD1_MASK;
        }
        if chord.ctrl {
            m |= CONTROL_MASK;
        }
        if chord.shift {
            m |= SHIFT_MASK;
        }
        if chord.super_key {
            m |= MOD4_MASK;
        }
        m
    }

    unsafe fn load_sym<T>(lib: *mut c_void, name: &[u8]) -> anyhow::Result<T> {
        let ptr = libc::dlsym(lib, name.as_ptr().cast::<c_char>());
        if ptr.is_null() {
            anyhow::bail!("Missing X11 symbol {:?}", std::str::from_utf8(name));
        }
        Ok(mem::transmute_copy(&ptr))
    }

    pub struct LinuxHotkeyGuard {
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }

    impl Drop for LinuxHotkeyGuard {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    pub fn install(
        copy: ParsedChord,
        paste: ParsedChord,
        tx: Sender<HotkeyAction>,
    ) -> anyhow::Result<LinuxHotkeyGuard> {
        if std::env::var_os("DISPLAY").is_none() {
            anyhow::bail!("DISPLAY environment variable not set");
        }

        let lib = unsafe { libc::dlopen(b"libX11.so.6\0".as_ptr().cast::<c_char>(), libc::RTLD_LAZY) };
        if lib.is_null() {
            anyhow::bail!("libX11.so.6 not available");
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let (init_tx, init_rx) = mpsc::channel::<anyhow::Result<()>>();

        let join = std::thread::spawn(move || unsafe {
            let res: anyhow::Result<(
                *mut c_void,
                c_ulong,
                c_int,
                c_int,
                c_uint,
                c_uint,
                FnXCloseDisplay,
                FnXUngrabKey,
                FnXPending,
                FnXNextEvent,
            )> = (|| {
                let x_open: FnXOpenDisplay = load_sym(lib, b"XOpenDisplay\0")?;
                let x_close: FnXCloseDisplay = load_sym(lib, b"XCloseDisplay\0")?;
                let x_root: FnXDefaultRootWindow = load_sym(lib, b"XDefaultRootWindow\0")?;
                let x_keysym: FnXKeysymToKeycode = load_sym(lib, b"XKeysymToKeycode\0")?;
                let x_grab: FnXGrabKey = load_sym(lib, b"XGrabKey\0")?;
                let x_ungrab: FnXUngrabKey = load_sym(lib, b"XUngrabKey\0")?;
                let x_pending: FnXPending = load_sym(lib, b"XPending\0")?;
                let x_next: FnXNextEvent = load_sym(lib, b"XNextEvent\0")?;
                let x_sync: FnXSync = load_sym(lib, b"XSync\0")?;

                let dpy = x_open(ptr::null());
                if dpy.is_null() {
                    anyhow::bail!("XOpenDisplay failed");
                }
                let root = x_root(dpy);
                let copy_kc = x_keysym(dpy, copy.key.to_ascii_lowercase() as c_ulong);
                let paste_kc = x_keysym(dpy, paste.key.to_ascii_lowercase() as c_ulong);
                if copy_kc == 0 || paste_kc == 0 {
                    x_close(dpy);
                    anyhow::bail!("XKeysymToKeycode failed");
                }

                let copy_mask = to_x11_mask(&copy);
                let paste_mask = to_x11_mask(&paste);
                for extra in [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
                    x_grab(dpy, copy_kc, copy_mask | extra, root, 0, GRAB_MODE_ASYNC, GRAB_MODE_ASYNC);
                    x_grab(dpy, paste_kc, paste_mask | extra, root, 0, GRAB_MODE_ASYNC, GRAB_MODE_ASYNC);
                }
                x_sync(dpy, 0);
                Ok((
                    dpy, root, copy_kc, paste_kc, copy_mask, paste_mask, x_close, x_ungrab,
                    x_pending, x_next,
                ))
            })();

            match res {
                Err(e) => {
                    libc::dlclose(lib);
                    let _ = init_tx.send(Err(e));
                }
                Ok((
                    dpy,
                    root,
                    copy_kc,
                    paste_kc,
                    copy_mask,
                    paste_mask,
                    x_close,
                    x_ungrab,
                    x_pending,
                    x_next,
                )) => {
                    let _ = init_tx.send(Ok(()));
                    let mut ev: XEvent = mem::zeroed();
                    while !stop_clone.load(Ordering::SeqCst) {
                        if x_pending(dpy) > 0 {
                            x_next(dpy, &mut ev);
                            if ev.type_ == KEY_PRESS {
                                let kc = ev.xkey.keycode as c_int;
                                let st = ev.xkey.state & !(LOCK_MASK | MOD2_MASK);
                                if kc == copy_kc && st == copy_mask {
                                    let _ = tx.send(HotkeyAction::BroadcastCopy);
                                } else if kc == paste_kc && st == paste_mask {
                                    let _ = tx.send(HotkeyAction::SmartPaste);
                                }
                            }
                        } else {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                    for extra in [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
                        x_ungrab(dpy, copy_kc, copy_mask | extra, root);
                        x_ungrab(dpy, paste_kc, paste_mask | extra, root);
                    }
                    x_close(dpy);
                    libc::dlclose(lib);
                }
            }
        });

        init_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Linux X11 hotkey thread terminated"))??;

        Ok(LinuxHotkeyGuard {
            stop,
            join: Some(join),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_chord_resolution_and_parsing() {
        let binding = GlobalHotkeyBinding::default();
        assert_eq!(binding.resolve_chord("Alt+C"), Some(HotkeyAction::BroadcastCopy));
        assert_eq!(binding.resolve_chord("alt+v"), Some(HotkeyAction::SmartPaste));
        assert_eq!(binding.resolve_chord("Ctrl+C"), None);

        let parsed_c = parse_hotkey_chord("Alt+C").unwrap();
        assert!(parsed_c.alt && !parsed_c.ctrl && parsed_c.key == 'C');
        let parsed_v = parse_hotkey_chord("Option+Shift+v").unwrap();
        assert!(parsed_v.alt && parsed_v.shift && parsed_v.key == 'V');
        assert!(parse_hotkey_chord("Alt+").is_none());
    }
}
