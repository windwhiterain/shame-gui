//! Windows pen/stylus input via direct `WM_POINTER` interception.
//!
//! winit 0.30 has no pen events of its own, and its `WM_POINTER → Touch`
//! conversion is unreliable for pens: on any system with a digitizer winit
//! calls `RegisterTouchWindow`, which reroutes pen input to `WM_TOUCH` — a
//! message that carries **no pressure** — and a failing
//! `GetPointerFrameInfoHistory` silently swallows the message. Instead of
//! depending on that path we replace the winit window procedure ourselves and
//! translate `WM_POINTER*` messages (PT_PEN) into [`PenInput`] events with
//! real `GetPointerPenInfo` pressure, delivered to the app as winit
//! `UserEvent`s. Non-pen pointers (touch screens) are translated the same way
//! with no pressure, so touch behavior is unchanged; the window is also
//! unregistered from `WM_TOUCH` so pen input actually reaches `WM_POINTER`.

use std::sync::Mutex;

use crate::gui::event::{InputEvent, MouseButton};
use crate::math::Vec2;

/// A pen/touch pointer event, delivered as a winit `Event::UserEvent`.
///
/// `pressure` is the normalized pen pressure (0.0–1.0), `None` for non-pen
/// pointers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenInput {
    Down { pos: Vec2, pressure: Option<f32> },
    Move { pos: Vec2, pressure: Option<f32> },
    Up { pos: Vec2 },
}

/// Events queued by the window-procedure interceptor (Windows only), drained
/// by the app in its `user_event` callback. Empty on other platforms.
static QUEUE: Mutex<Vec<PenInput>> = Mutex::new(Vec::new());

pub(crate) fn push_event(event: PenInput) {
    QUEUE.lock().unwrap().push(event);
}

pub(crate) fn take_events() -> Vec<PenInput> {
    std::mem::take(&mut *QUEUE.lock().unwrap())
}

/// Converts a pen event to the widget-facing input event.
pub(crate) fn to_input(event: PenInput) -> InputEvent {
    match event {
        PenInput::Down { pos, pressure } => InputEvent::MouseDown {
            pos,
            button: MouseButton::Left,
            pressure,
        },
        PenInput::Move { pos, pressure } => InputEvent::MouseMove { pos, pressure },
        PenInput::Up { pos } => InputEvent::MouseUp {
            pos,
            button: MouseButton::Left,
            pressure: None,
        },
    }
}

#[cfg(windows)]
pub(crate) mod imp {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, WPARAM};
    use windows_sys::Win32::UI::Input::Pointer::{
        GetPointerInfo, GetPointerPenInfo, POINTER_FLAG_CANCELED, POINTER_INFO, POINTER_PEN_INFO,
    };
    use windows_sys::Win32::UI::Input::Touch::UnregisterTouchWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, PT_PEN, SetWindowLongPtrW, WM_NCDESTROY,
        WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE, WNDPROC,
    };
    use winit::event_loop::EventLoopProxy;

    use super::{PenInput, push_event};
    use crate::math::Vec2;

    // windows-sys 0.52 does not bind ScreenToClient; declare it directly.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ScreenToClient(hwnd: HWND, lppoint: *mut POINT) -> BOOL;
    }

    struct PenState {
        proxy: EventLoopProxy<()>,
    }

    static PEN_STATE: Mutex<Option<PenState>> = Mutex::new(None);

    /// The original window procedure. Lock-free on purpose: `wnd_proc` must
    /// not hold `PEN_STATE` while calling into it — Windows runs modal
    /// message loops (title-bar drag, edge resize, the system menu) *inside*
    /// `DefWindowProc`, dispatching messages that re-enter `wnd_proc` on the
    /// same thread, and a same-thread relock of the mutex would deadlock the
    /// window. The pointer is set once at install and cleared at
    /// `WM_NCDESTROY`.
    static OLD_PROC: AtomicUsize = AtomicUsize::new(0);

    /// Installs the `WM_POINTER` interceptor on `hwnd`. Call once, right after
    /// the window is created and before any pen contact.
    pub(crate) fn install(hwnd: HWND, proxy: EventLoopProxy<()>) {
        // winit registers the window for WM_TOUCH whenever a digitizer is
        // present; that reroutes pen input away from WM_POINTER (losing
        // pressure). Unregister so pen (and touch) input arrives as
        // WM_POINTER instead.
        unsafe { UnregisterTouchWindow(hwnd) };

        let old_proc = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
        let old_proc: WNDPROC = unsafe { std::mem::transmute(old_proc) };
        OLD_PROC.store(
            unsafe { std::mem::transmute::<WNDPROC, usize>(old_proc) },
            Ordering::Relaxed,
        );
        *PEN_STATE.lock().unwrap() = Some(PenState { proxy });
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as isize) };
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> isize {
        match msg {
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
                let pointer_id = (wparam & 0xffff) as u32;
                let mut info: POINTER_INFO = unsafe { std::mem::zeroed() };
                if unsafe { GetPointerInfo(pointer_id, &mut info) } != 0 {
                    // Only pens carry pressure; touch pointers get `None` so
                    // they keep behaving exactly like mouse input.
                    let pressure = if info.pointerType == PT_PEN {
                        let mut pen: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
                        if unsafe { GetPointerPenInfo(pointer_id, &mut pen) } != 0 {
                            Some((pen.pressure as f32 / 1024.0).clamp(0.0, 1.0))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let mut pt = info.ptPixelLocation;
                    if unsafe { ScreenToClient(hwnd, &mut pt) } != 0 {
                        let pos = Vec2::new(pt.x as f32, pt.y as f32);
                        // Phase comes from the message type itself, not the
                        // pointer flags — drivers are not guaranteed to set
                        // POINTER_FLAG_DOWN in `GetPointerInfo`'s reply.
                        let canceled = info.pointerFlags & POINTER_FLAG_CANCELED != 0;
                        let event = match msg {
                            WM_POINTERDOWN => PenInput::Down { pos, pressure },
                            WM_POINTERUP => PenInput::Up { pos },
                            _ if canceled => PenInput::Up { pos },
                            _ => PenInput::Move { pos, pressure },
                        };
                        push_event(event);
                        if let Some(state) = PEN_STATE.lock().unwrap().as_ref() {
                            let _ = state.proxy.send_event(());
                        }
                        // Handled: winit never sees the message, and Windows
                        // does not promote it to mouse input either.
                        return 0;
                    }
                }
            }
            WM_POINTERLEAVE => {
                // Mirror the CursorLeft convention: park the cursor far away.
                push_event(PenInput::Move {
                    pos: Vec2::new(-10000.0, -10000.0),
                    pressure: None,
                });
                if let Some(state) = PEN_STATE.lock().unwrap().as_ref() {
                    let _ = state.proxy.send_event(());
                }
                return 0;
            }
            WM_NCDESTROY => {
                let _state = PEN_STATE.lock().unwrap().take();
                let old = OLD_PROC.swap(0, Ordering::Relaxed);
                if old == 0 {
                    return 0;
                }
                let old: WNDPROC = unsafe { std::mem::transmute(old) };
                return unsafe { CallWindowProcW(old, hwnd, msg, wparam, lparam) };
            }
            _ => {}
        }
        // Every other message goes to the original procedure. The pointer is
        // read without taking `PEN_STATE` — see `OLD_PROC` for why the lock
        // must not be held across `CallWindowProcW`.
        let old = OLD_PROC.load(Ordering::Relaxed);
        if old == 0 {
            return 0;
        }
        let old: WNDPROC = unsafe { std::mem::transmute(old) };
        unsafe { CallWindowProcW(old, hwnd, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_down_maps_to_mouse_down_with_pressure() {
        assert_eq!(
            to_input(PenInput::Down {
                pos: Vec2::new(10.0, 20.0),
                pressure: Some(0.5),
            }),
            InputEvent::MouseDown {
                pos: Vec2::new(10.0, 20.0),
                button: MouseButton::Left,
                pressure: Some(0.5),
            }
        );
    }

    #[test]
    fn pen_move_maps_to_mouse_move() {
        assert_eq!(
            to_input(PenInput::Move {
                pos: Vec2::new(30.0, 40.0),
                pressure: None,
            }),
            InputEvent::MouseMove {
                pos: Vec2::new(30.0, 40.0),
                pressure: None,
            }
        );
    }

    #[test]
    fn pen_up_maps_to_mouse_up() {
        assert_eq!(
            to_input(PenInput::Up {
                pos: Vec2::new(30.0, 40.0),
            }),
            InputEvent::MouseUp {
                pos: Vec2::new(30.0, 40.0),
                button: MouseButton::Left,
                pressure: None,
            }
        );
    }
}
