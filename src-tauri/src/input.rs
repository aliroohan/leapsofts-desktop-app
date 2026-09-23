use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static KEY_THIS_SEC: AtomicBool = AtomicBool::new(false);
static MOUSE_THIS_SEC: AtomicBool = AtomicBool::new(false);
static LAST_KEY: AtomicBool = AtomicBool::new(false);
static LAST_MOUSE: AtomicBool = AtomicBool::new(false);
static HOOK_STARTED: AtomicBool = AtomicBool::new(false);
static CUMULATIVE_ACTIVE: AtomicU64 = AtomicU64::new(0);
static TICK_STARTED: AtomicBool = AtomicBool::new(false);

fn mark_key() {
    KEY_THIS_SEC.store(true, Ordering::Relaxed);
}

fn mark_mouse() {
    MOUSE_THIS_SEC.store(true, Ordering::Relaxed);
}

fn start_tick_loop() {
    if TICK_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(1));
        let key = KEY_THIS_SEC.swap(false, Ordering::Relaxed);
        let mouse = MOUSE_THIS_SEC.swap(false, Ordering::Relaxed);
        LAST_KEY.store(key, Ordering::Relaxed);
        LAST_MOUSE.store(mouse, Ordering::Relaxed);
        if key || mouse {
            CUMULATIVE_ACTIVE.fetch_add(1, Ordering::Relaxed);
        }
    });
}

pub fn ensure_started() -> bool {
    start_tick_loop();

    if HOOK_STARTED.load(Ordering::SeqCst) {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        if !macos_accessibility_client::accessibility::application_is_trusted_with_prompt() {
            return false;
        }
        HOOK_STARTED.store(true, Ordering::SeqCst);
        thread::spawn(|| {
            if !macos_tap::run_listen_only_tap() {
                HOOK_STARTED.store(false, Ordering::SeqCst);
            }
        });
        return true;
    }

    #[cfg(not(target_os = "macos"))]
    {
        HOOK_STARTED.store(true, Ordering::SeqCst);
        thread::spawn(|| {
            let _ = std::panic::catch_unwind(|| {
                if rdev::listen(on_rdev_event).is_err() {
                    HOOK_STARTED.store(false, Ordering::SeqCst);
                }
            });
            HOOK_STARTED.store(false, Ordering::SeqCst);
        });
        true
    }
}

#[cfg(not(target_os = "macos"))]
fn on_rdev_event(event: rdev::Event) {
    match event.event_type {
        rdev::EventType::KeyPress(_) | rdev::EventType::KeyRelease(_) => mark_key(),
        rdev::EventType::MouseMove { .. }
        | rdev::EventType::ButtonPress(_)
        | rdev::EventType::ButtonRelease(_)
        | rdev::EventType::Wheel { .. } => mark_mouse(),
    }
}

pub fn on_tick() -> (bool, bool) {
    (
        LAST_KEY.load(Ordering::Relaxed),
        LAST_MOUSE.load(Ordering::Relaxed),
    )
}

pub fn last_tick() -> (bool, bool) {
    on_tick()
}

pub fn get_cumulative_active_sec() -> u64 {
    CUMULATIVE_ACTIVE.load(Ordering::Relaxed)
}

pub fn has_pending_input_this_second() -> bool {
    KEY_THIS_SEC.load(Ordering::Relaxed) || MOUSE_THIS_SEC.load(Ordering::Relaxed)
}

/// Listen-only HID event tap. Counts key/mouse events only — no unicode/TSM mapping.
#[cfg(target_os = "macos")]
mod macos_tap {
    use super::{mark_key, mark_mouse};
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicPtr, Ordering};

    type CGEventTapProxy = *mut c_void;
    type CGEventRef = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFStringRef = *const c_void;

    const K_CGHID_EVENT_TAP: u32 = 0;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
    const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
    const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
    const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
    const K_CG_EVENT_KEY_DOWN: u32 = 10;
    const K_CG_EVENT_KEY_UP: u32 = 11;
    const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
    const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
    const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
    const K_CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
    const K_CG_EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

    static TAP_PORT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: CFStringRef;
    }

    fn mask_bit(event_type: u32) -> u64 {
        1u64 << event_type
    }

    fn events_of_interest() -> u64 {
        mask_bit(K_CG_EVENT_LEFT_MOUSE_DOWN)
            | mask_bit(K_CG_EVENT_LEFT_MOUSE_UP)
            | mask_bit(K_CG_EVENT_RIGHT_MOUSE_DOWN)
            | mask_bit(K_CG_EVENT_RIGHT_MOUSE_UP)
            | mask_bit(K_CG_EVENT_MOUSE_MOVED)
            | mask_bit(K_CG_EVENT_LEFT_MOUSE_DRAGGED)
            | mask_bit(K_CG_EVENT_RIGHT_MOUSE_DRAGGED)
            | mask_bit(K_CG_EVENT_KEY_DOWN)
            | mask_bit(K_CG_EVENT_KEY_UP)
            | mask_bit(K_CG_EVENT_FLAGS_CHANGED)
            | mask_bit(K_CG_EVENT_SCROLL_WHEEL)
            | mask_bit(K_CG_EVENT_OTHER_MOUSE_DOWN)
            | mask_bit(K_CG_EVENT_OTHER_MOUSE_UP)
            | mask_bit(K_CG_EVENT_OTHER_MOUSE_DRAGGED)
    }

    extern "C" fn tap_callback(
        _proxy: CGEventTapProxy,
        etype: u32,
        event: CGEventRef,
        _info: *mut c_void,
    ) -> CGEventRef {
        if etype == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || etype == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            let port = TAP_PORT.load(Ordering::SeqCst);
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
            return event;
        }
        match etype {
            K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP | K_CG_EVENT_FLAGS_CHANGED => mark_key(),
            K_CG_EVENT_LEFT_MOUSE_DOWN
            | K_CG_EVENT_LEFT_MOUSE_UP
            | K_CG_EVENT_RIGHT_MOUSE_DOWN
            | K_CG_EVENT_RIGHT_MOUSE_UP
            | K_CG_EVENT_MOUSE_MOVED
            | K_CG_EVENT_LEFT_MOUSE_DRAGGED
            | K_CG_EVENT_RIGHT_MOUSE_DRAGGED
            | K_CG_EVENT_SCROLL_WHEEL
            | K_CG_EVENT_OTHER_MOUSE_DOWN
            | K_CG_EVENT_OTHER_MOUSE_UP
            | K_CG_EVENT_OTHER_MOUSE_DRAGGED => mark_mouse(),
            _ => {}
        }
        event
    }

    /// Blocks on this thread's CFRunLoop. Returns false if the tap could not be created.
    pub fn run_listen_only_tap() -> bool {
        unsafe {
            let tap = CGEventTapCreate(
                K_CGHID_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                events_of_interest(),
                tap_callback,
                ptr::null_mut(),
            );
            if tap.is_null() {
                return false;
            }
            TAP_PORT.store(tap, Ordering::SeqCst);
            let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
            if source.is_null() {
                TAP_PORT.store(ptr::null_mut(), Ordering::SeqCst);
                return false;
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRun();
        }
        false
    }
}
