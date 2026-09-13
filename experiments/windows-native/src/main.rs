//! DBM's Windows-native frontend: Win32 windowing with a custom Direct2D
//! renderer over the shared `dbm-engine` crate.
//!
//! This is an in-development experiment, not a replacement for the shipping
//! Tauri app. See `docs/native-platforms.md` for status and parity notes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod platform;
mod render;
mod theme;
mod ui;

use std::sync::Arc;

use dbm_engine::state::AppState;
use windows::core::{w, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_CHAR, WM_CLOSE,
    WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

mod satoshi {
    include!(concat!(env!("OUT_DIR"), "/satoshi.rs"));
}

const TIMER_ID: usize = 1;

struct App {
    renderer: render::Renderer,
    ui: ui::Ui,
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class_name = w!("DbmNativeWindow");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(instance.0),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32());
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("DBM"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1400,
            900,
            None,
            None,
            HINSTANCE(instance.0),
            None,
        )?;

        let engine = Arc::new(AppState::new().map_err(|error| {
            windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                format!("DBM local storage must initialize: {error}"),
            )
        })?);
        bridge::install(engine, hwnd.0 as isize);

        let app = Box::new(App {
            renderer: render::Renderer::new(hwnd)?,
            ui: ui::Ui::new(),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        SetTimer(hwnd, TIMER_ID, 120, None);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn with_app<R>(hwnd: HWND, f: impl FnOnce(&mut App) -> R) -> Option<R> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if pointer.is_null() {
        return None;
    }
    Some(f(unsafe { &mut *pointer }))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut paint);
            with_app(hwnd, |app| {
                let dpi = GetDpiForWindow(hwnd) as f32;
                app.renderer.set_dpi(if dpi <= 0.0 { 96.0 } else { dpi });
                let (width, height) = client_size(hwnd);
                app.ui.time = now();
                let App { renderer, ui } = app;
                if renderer.begin(width, height).unwrap_or(false) {
                    ui.paint(renderer);
                    let _ = renderer.end();
                }
            });
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_SIZE => {
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;
            if !info.is_null() {
                (*info).ptMinTrackSize.x = 960;
                (*info).ptMinTrackSize.y = 640;
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point_from(lparam);
            with_app(hwnd, |app| {
                let scale = app.renderer.dpi_scale();
                app.ui.on_mouse_move(x / scale, y / scale);
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point_from(lparam);
            with_app(hwnd, |app| {
                let scale = app.renderer.dpi_scale();
                let App { renderer, ui } = app;
                ui.on_mouse_down(renderer, x / scale, y / scale);
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point_from(lparam);
            with_app(hwnd, |app| {
                let scale = app.renderer.dpi_scale();
                let App { renderer, ui } = app;
                ui.on_mouse_up(
                    renderer,
                    x / scale,
                    y / scale,
                    key_down(VK_CONTROL.0 as i32),
                    key_down(VK_SHIFT.0 as i32),
                );
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = point_from(lparam);
            with_app(hwnd, |app| {
                let scale = app.renderer.dpi_scale();
                app.ui.on_double_click(x / scale, y / scale);
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as u16 as i16 as f32 / 120.0 * 60.0;
            let mut point = windows::Win32::Foundation::POINT {
                x: (lparam.0 & 0xffff) as u16 as i16 as i32,
                y: ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32,
            };
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point);
            with_app(hwnd, |app| {
                let scale = app.renderer.dpi_scale();
                app.ui
                    .on_wheel(point.x as f32 / scale, point.y as f32 / scale, delta);
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CHAR => {
            let code = wparam.0 as u32;
            if let Some(character) = char::from_u32(code) {
                with_app(hwnd, |app| app.ui.on_char(character));
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u32;
            let control = key_down(VK_CONTROL.0 as i32);
            let shift = key_down(VK_SHIFT.0 as i32);
            let handled = with_app(hwnd, |app| {
                let App { renderer, ui } = app;
                if control && ui.handle_shortcut(renderer, key) {
                    return true;
                }
                ui.on_key(renderer, key, shift, control)
            })
            .unwrap_or(false);
            if handled {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let events = bridge::drain();
            let had_events = !events.is_empty();
            with_app(hwnd, |app| {
                for event in events {
                    app.ui.handle_event(event);
                }
                app.ui.time = now();
            });
            let animated = with_app(hwnd, |app| {
                app.ui.toast.is_some()
                    || app.ui.error.is_some()
                    || app.ui.tabs.iter().any(|tab| {
                        app.ui
                            .tables
                            .get(&tab.id)
                            .is_some_and(|state| state.loading)
                            || app
                                .ui
                                .queries
                                .get(&tab.id)
                                .is_some_and(|state| state.running)
                    })
            })
            .unwrap_or(false);
            if had_events || animated {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !pointer.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            }
            KillTimer(hwnd, TIMER_ID).ok();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rect = windows::Win32::Foundation::RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    (
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    )
}

fn point_from(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xffff) as u16 as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as f32;
    (x, y)
}

fn key_down(key: i32) -> bool {
    unsafe { (GetKeyState(key) as u16 & 0x8000) != 0 }
}

fn now() -> f32 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() as f32 / 1000.0 }
}
