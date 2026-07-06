//! Native Win32 splash window. Shown before the WebView is ready so the
//! user has visual feedback while the React UI loads. Single dedicated
//! thread with its own message loop; main thread posts WM_CLOSE on drop
//! to tear it down.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect, SelectObject, SetBkMode,
    SetTextColor, UpdateWindow, DRAW_TEXT_FORMAT, DT_CENTER, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
    FW_BOLD, FW_NORMAL, HBRUSH, HDC, HGDIOBJ, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics, KillTimer,
    LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SetLayeredWindowAttributes,
    SetTimer, ShowWindow, TranslateMessage, IDC_ARROW, LWA_ALPHA, MSG, SM_CXSCREEN, SM_CYSCREEN,
    SW_SHOW, WM_APP, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};

const CLASS_NAME: PCWSTR = w!("CodexUpdaterSplash");
const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 16;
const WM_SET_STATUS: u32 = WM_APP + 1;

const LOGICAL_W: i32 = 636;
const LOGICAL_H: i32 = 499;
const BAR_W: i32 = 542;
const BAR_H: i32 = 10;
const BAR_FG_W: i32 = 170;
const TITLEBAR_H: i32 = 31;

// COLORREF byte order is 0x00BBGGRR.
const COLOR_BG: u32 = 0x00141915; // #151914
const COLOR_TITLEBAR: u32 = 0x00f3f3f3; // #f3f3f3
const COLOR_TITLEBAR_TEXT: u32 = 0x00555555; // #555555
const COLOR_WINDOW_ICON: u32 = 0x000082d8; // #d88200
const COLOR_TEXT: u32 = 0x00e8f1f4; // #f4f1e8
const COLOR_DIM: u32 = 0x0098a3a7; // #a7a398
const COLOR_DIVIDER: u32 = 0x00333731; // #313733
const COLOR_BAR_BG: u32 = 0x00272d2a; // #2a2d27
const COLOR_BAR_FG: u32 = 0x00c7d166; // #66d1c7

pub struct Splash {
    hwnd: usize,
}

// HWND messaging via PostMessageW is documented thread-safe.
unsafe impl Send for Splash {}

impl Splash {
    /// Spawn a splash on a dedicated thread. Returns once CreateWindowEx
    /// completes (HWND back via channel). Returns None if the window failed.
    pub fn show(_codex_exe: Option<PathBuf>) -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || unsafe {
            run_splash(tx);
        });
        let hwnd = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .ok()
            .filter(|h| *h != 0)?;
        Some(Self { hwnd })
    }

    pub fn set_status(&self, status: &str) {
        if self.hwnd == 0 {
            return;
        }
        let status = Box::new(status.to_string());
        unsafe {
            let _ = PostMessageW(
                HWND(self.hwnd as *mut _),
                WM_SET_STATUS,
                WPARAM(0),
                LPARAM(Box::into_raw(status) as isize),
            );
        }
    }
}

impl Drop for Splash {
    fn drop(&mut self) {
        if self.hwnd != 0 {
            unsafe {
                let _ = PostMessageW(HWND(self.hwnd as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

thread_local! {
    static BAR_OFFSET: Cell<i32> = const { Cell::new(0) };
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
    static STATUS_TEXT: RefCell<String> = RefCell::new("正在检查".to_string());
}

unsafe fn run_splash(tx: mpsc::Sender<usize>) {
    let hinstance = match GetModuleHandleW(None) {
        Ok(h) => h,
        Err(_) => {
            let _ = tx.send(0);
            return;
        }
    };
    let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();

    let hinstance: HINSTANCE = HINSTANCE(hinstance.0);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: CLASS_NAME,
        hCursor: cursor,
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let scale = GetDpiForSystem() as f32 / 96.0;
    SCALE.with(|s| s.set(scale));
    let w = (LOGICAL_W as f32 * scale) as i32;
    let h = (LOGICAL_H as f32 * scale) as i32;
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let x = ((screen_w - w) / 2).max(0);
    let y = ((screen_h - h) / 2).max(0);

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
        CLASS_NAME,
        w!("Codex Windows 中文助手"),
        WS_POPUP,
        x,
        y,
        w,
        h,
        HWND::default(),
        None,
        hinstance,
        None,
    );
    let hwnd = match hwnd {
        Ok(h) if !h.is_invalid() => h,
        _ => {
            let _ = tx.send(0);
            return;
        }
    };

    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = UpdateWindow(hwnd);
    let _ = SetTimer(hwnd, TIMER_ID, TIMER_MS, None);

    let _ = tx.send(hwnd.0 as usize);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SET_STATUS => {
            let ptr = lp.0 as *mut String;
            if !ptr.is_null() {
                let status = Box::from_raw(ptr);
                STATUS_TEXT.with(|s| *s.borrow_mut() = *status);
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            BAR_OFFSET.with(|c| c.set((c.get() + 6) % (BAR_W + BAR_FG_W)));
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_ID);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, _wp, lp),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let scale = SCALE.with(|s| s.get());
    let w = (LOGICAL_W as f32 * scale) as i32;
    let h = (LOGICAL_H as f32 * scale) as i32;

    let mem_dc = CreateCompatibleDC(hdc);
    let mem_bitmap = CreateCompatibleBitmap(hdc, w, h);
    if mem_dc.is_invalid() || mem_bitmap.is_invalid() {
        paint_content(hdc, scale);
        if !mem_bitmap.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(mem_bitmap.0));
        }
        if !mem_dc.is_invalid() {
            let _ = DeleteDC(mem_dc);
        }
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let old_bitmap = SelectObject(mem_dc, HGDIOBJ(mem_bitmap.0));
    paint_content(mem_dc, scale);
    let _ = BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    if !old_bitmap.0.is_null() {
        SelectObject(mem_dc, old_bitmap);
    }
    let _ = DeleteObject(HGDIOBJ(mem_bitmap.0));
    let _ = DeleteDC(mem_dc);

    let _ = EndPaint(hwnd, &ps);
}

unsafe fn paint_content(hdc: HDC, scale: f32) {
    let scaled = |v: i32| (v as f32 * scale) as i32;
    let w = scaled(LOGICAL_W);
    let h = scaled(LOGICAL_H);
    let bar_w = scaled(BAR_W);
    let bar_h = scaled(BAR_H);
    let bar_fg_w = scaled(BAR_FG_W);

    let titlebar_brush = CreateSolidBrush(COLORREF(COLOR_TITLEBAR));
    FillRect(
        hdc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: scaled(TITLEBAR_H),
        },
        titlebar_brush,
    );
    let _ = DeleteObject(HGDIOBJ(titlebar_brush.0));

    let icon_brush = CreateSolidBrush(COLORREF(COLOR_WINDOW_ICON));
    FillRect(
        hdc,
        &RECT {
            left: scaled(8),
            top: scaled(8),
            right: scaled(24),
            bottom: scaled(24),
        },
        icon_brush,
    );
    let _ = DeleteObject(HGDIOBJ(icon_brush.0));

    let bg_brush = CreateSolidBrush(COLORREF(COLOR_BG));
    FillRect(
        hdc,
        &RECT {
            left: 0,
            top: scaled(TITLEBAR_H),
            right: w,
            bottom: h,
        },
        bg_brush,
    );
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    SetBkMode(hdc, TRANSPARENT);

    let title_font = CreateFontW(
        scaled(20),
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Microsoft YaHei UI"),
    );
    let meta_font = CreateFontW(
        scaled(12),
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Microsoft YaHei UI"),
    );
    let body_font = CreateFontW(
        scaled(19),
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Microsoft YaHei UI"),
    );
    let titlebar_font = CreateFontW(
        scaled(13),
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Microsoft YaHei UI"),
    );

    let prev_font = SelectObject(hdc, HGDIOBJ(titlebar_font.0));
    SetTextColor(hdc, COLORREF(COLOR_TITLEBAR_TEXT));
    draw_text(
        hdc,
        "Codex Windows 中文助手",
        scaled(30),
        0,
        scaled(260),
        scaled(TITLEBAR_H),
        DRAW_TEXT_FORMAT(0),
    );
    draw_text(
        hdc,
        "-",
        w - scaled(124),
        0,
        w - scaled(104),
        scaled(TITLEBAR_H),
        DT_CENTER,
    );
    draw_text(
        hdc,
        "□",
        w - scaled(78),
        0,
        w - scaled(58),
        scaled(TITLEBAR_H),
        DT_CENTER,
    );
    draw_text(
        hdc,
        "×",
        w - scaled(34),
        0,
        w - scaled(14),
        scaled(TITLEBAR_H),
        DT_CENTER,
    );

    SelectObject(hdc, HGDIOBJ(title_font.0));
    SetTextColor(hdc, COLORREF(COLOR_TEXT));
    draw_text(
        hdc,
        "Codex Windows 中文助手",
        scaled(34),
        scaled(62),
        w - scaled(34),
        scaled(90),
        DRAW_TEXT_FORMAT(0),
    );

    SelectObject(hdc, HGDIOBJ(meta_font.0));
    SetTextColor(hdc, COLORREF(COLOR_DIM));
    draw_text(
        hdc,
        "启动中",
        scaled(34),
        scaled(94),
        w - scaled(34),
        scaled(114),
        DRAW_TEXT_FORMAT(0),
    );
    let status = STATUS_TEXT.with(|s| s.borrow().clone());
    draw_text(
        hdc,
        &status,
        scaled(320),
        scaled(62),
        w - scaled(34),
        scaled(90),
        DT_RIGHT,
    );

    let divider_brush = CreateSolidBrush(COLORREF(COLOR_DIVIDER));
    FillRect(
        hdc,
        &RECT {
            left: scaled(34),
            top: scaled(119),
            right: w - scaled(34),
            bottom: scaled(120),
        },
        divider_brush,
    );
    let _ = DeleteObject(HGDIOBJ(divider_brush.0));

    SelectObject(hdc, HGDIOBJ(body_font.0));
    SetTextColor(hdc, COLORREF(COLOR_TEXT));
    draw_text(
        hdc,
        "正在检查更新",
        scaled(40),
        scaled(225),
        w - scaled(40),
        scaled(257),
        DT_CENTER,
    );

    let bar_y = scaled(264);
    let bar_x = (w - bar_w) / 2;
    let bar_bg = CreateSolidBrush(COLORREF(COLOR_BAR_BG));
    FillRect(
        hdc,
        &RECT {
            left: bar_x,
            top: bar_y,
            right: bar_x + bar_w,
            bottom: bar_y + bar_h,
        },
        bar_bg,
    );
    let _ = DeleteObject(HGDIOBJ(bar_bg.0));

    let raw = BAR_OFFSET.with(|c| c.get());
    let span = BAR_W + BAR_FG_W;
    let local = raw % span - BAR_FG_W;
    let pos = scaled(local);
    let fg_left = (bar_x + pos).max(bar_x);
    let fg_right = (bar_x + pos + bar_fg_w).min(bar_x + bar_w);
    if fg_right > fg_left {
        let bar_fg = CreateSolidBrush(COLORREF(COLOR_BAR_FG));
        FillRect(
            hdc,
            &RECT {
                left: fg_left,
                top: bar_y,
                right: fg_right,
                bottom: bar_y + bar_h,
            },
            bar_fg,
        );
        let _ = DeleteObject(HGDIOBJ(bar_fg.0));
    }

    SelectObject(hdc, HGDIOBJ(meta_font.0));
    SetTextColor(hdc, COLORREF(COLOR_DIM));
    draw_text(
        hdc,
        "准备中",
        scaled(40),
        scaled(289),
        w - scaled(40),
        scaled(311),
        DT_CENTER,
    );
    SetTextColor(hdc, COLORREF(COLOR_TEXT));
    draw_text(
        hdc,
        "正在读取安装、更新和启动器状态。",
        scaled(40),
        scaled(323),
        w - scaled(40),
        scaled(347),
        DT_CENTER,
    );

    SelectObject(hdc, prev_font);
    let _ = DeleteObject(HGDIOBJ(title_font.0));
    let _ = DeleteObject(HGDIOBJ(meta_font.0));
    let _ = DeleteObject(HGDIOBJ(body_font.0));
    let _ = DeleteObject(HGDIOBJ(titlebar_font.0));
}

unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    flags: DRAW_TEXT_FORMAT,
) {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT {
        left,
        top,
        right,
        bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut rect,
        flags | DT_SINGLELINE | DT_VCENTER,
    );
}
