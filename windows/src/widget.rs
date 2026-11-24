//! A small glass card on the desktop with the weather outside: the temperature now, what the sky is doing and where,
//! and the next six hours. Drawn with Direct2D into a bitmap a layered window shows with per-pixel alpha; dragged
//! anywhere with the mouse, and kept where it was put.

use std::cell::RefCell;
use std::mem::size_of;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory,
    ID2D1Factory, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_LIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapCacheOnLoad};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};
use windows_numerics::Vector2;

use crate::log;
use crate::text::t;

const CLASS: PCWSTR = w!("DesktopFloraWidget");
/// The card in DIPs, and the room around it for its shadow.
const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 158.0;
const MARGIN: f32 = 22.0;

/// What the card shows.
#[derive(Clone, PartialEq)]
pub struct Report {
    pub temperature: f32,
    pub code: u32,
    pub day: bool,
    pub condition: String,
    pub place: String,
    /// The hours ahead: the hour of the day, the temperature and the WMO code.
    pub hours: Vec<(u8, f32, u8)>,
}

struct Widget {
    window: HWND,
    scale: f32,
    report: Report,
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

/// Shows the card with this report, opening it where it was last put, or bottom right.
pub fn show(report: Report) {
    let open = WIDGET.with_borrow(|widget| widget.as_ref().map(|widget| (widget.window, widget.scale, widget.report == report)));
    match open {
        Some((_, _, true)) => {}
        Some((window, scale, false)) => {
            WIDGET.with_borrow_mut(|widget| {
                if let Some(widget) = widget {
                    widget.report = report.clone();
                }
            });
            draw(window, scale, &report, None);
        }
        None => {
            if let Err(error) = create(report) {
                log!("weather widget failed: {error}");
            }
        }
    }
}

pub fn hide() {
    if let Some(widget) = WIDGET.take() {
        unsafe {
            let _ = DestroyWindow(widget.window);
        }
    }
}

fn create(report: Report) -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let (scale, origin) = placement();
        let size = pixels(scale);
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("DesktopFlora"),
            WS_POPUP,
            origin.x,
            origin.y,
            size.0,
            size.1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        WIDGET.set(Some(Widget { window, scale, report: report.clone() }));
        draw(window, scale, &report, Some(origin));
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // Anywhere on it, it can be dragged.
        WM_NCHITTEST => LRESULT(HTCAPTION as isize),
        WM_EXITSIZEMOVE => {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(window, &mut rect) }.is_ok() {
                crate::prefs::set_widget_position(Some((rect.left, rect.top)));
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn pixels(scale: f32) -> (i32, i32) {
    (((WIDTH + 2.0 * MARGIN) * scale).ceil() as i32, ((HEIGHT + 2.0 * MARGIN) * scale).ceil() as i32)
}

/// Where the card goes: where it was put last if that is still on a display, else bottom right of the main one.
fn placement() -> (f32, POINT) {
    unsafe {
        let saved = crate::prefs::widget_position();
        let point = saved.map_or(POINT { x: 0, y: 0 }, |(x, y)| POINT { x, y });
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi, mut _y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y);
        let scale = dpi.max(96) as f32 / 96.0;
        let size = pixels(scale);
        let work = info.rcWork;
        let on_screen = saved.is_some_and(|(x, y)| x >= work.left - size.0 / 2 && x < work.right - size.0 / 2 && y >= work.top && y < work.bottom - size.1 / 2);
        let origin = if on_screen { point } else { POINT { x: work.right - size.0 - (12.0 * scale) as i32, y: work.bottom - size.1 - (12.0 * scale) as i32 } };
        (scale, origin)
    }
}

fn draw(window: HWND, scale: f32, report: &Report, position: Option<POINT>) {
    match paint(scale, report) {
        Ok(bitmap) => snowhush::card::present(window, pixels(scale), &bitmap, position),
        Err(error) => log!("weather widget paint failed: {error}"),
    }
}

/// The sign for a kind of weather, by its WMO code, by day or by night.
fn sign(code: u32, day: bool) -> &'static str {
    match code {
        0 | 1 => {
            if day {
                "☀"
            } else {
                "☾"
            }
        }
        2 => "⛅",
        3 => "☁",
        45 | 48 => "≋",
        51..=67 | 80..=82 => "☂",
        71..=77 | 85 | 86 => "❄",
        95..=99 => "⚡",
        _ => "☁",
    }
}

fn paint(scale: f32, report: &Report) -> Result<Vec<u8>> {
    let (width, height) = pixels(scale);
    unsafe {
        let imaging: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let text: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let bitmap = imaging.CreateBitmap(width as u32, height as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * scale,
            dpiY: 96.0 * scale,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = d2d.CreateWicBitmapRenderTarget(&bitmap, &properties)?;
        target.BeginDraw();
        target.Clear(Some(&color([0.0; 3], 0.0)));
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let drawn = card(&target, &text, report);
        target.EndDraw(None, None)?;
        drawn?;
        drop(target);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        bitmap.CopyPixels(std::ptr::null(), (width * 4) as u32, &mut pixels)?;
        Ok(pixels)
    }
}

fn card(target: &ID2D1RenderTarget, text: &IDWriteFactory, report: &Report) -> Result<()> {
    let frame = D2D_RECT_F { left: MARGIN, top: MARGIN, right: MARGIN + WIDTH, bottom: MARGIN + HEIGHT };
    let brush = |rgb: [f32; 3], alpha: f32| unsafe { target.CreateSolidColorBrush(&color(rgb, alpha), None) };
    let write = |words: &str, font: PCWSTR, size: f32, weight: DWRITE_FONT_WEIGHT, rect: D2D_RECT_F, align: DWRITE_TEXT_ALIGNMENT, rgb: [f32; 3], alpha: f32| -> Result<()> {
        unsafe {
            let format = text.CreateTextFormat(font, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!(""))?;
            format.SetTextAlignment(align)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let units: Vec<u16> = words.encode_utf16().collect();
            target.DrawText(&units, &format, &rect, &brush(rgb, alpha)?, D2D1_DRAW_TEXT_OPTIONS_NONE, Default::default());
        }
        Ok(())
    };
    unsafe {
        // A soft shadow under frosted dark glass, a fine light edge round it.
        let shade = brush([0.0; 3], 0.03)?;
        for step in 0..12 {
            let spread = step as f32 * 1.5;
            let rect = D2D_RECT_F { left: frame.left - spread, top: frame.top - spread + 6.0, right: frame.right + spread, bottom: frame.bottom + spread + 6.0 };
            target.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: 18.0 + spread, radiusY: 18.0 + spread }, &shade);
        }
        let shape = D2D1_ROUNDED_RECT { rect: frame, radiusX: 18.0, radiusY: 18.0 };
        target.FillRoundedRectangle(&shape, &brush([0.07, 0.09, 0.13], 0.8)?);
        let sheen = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { bottom: frame.top + HEIGHT * 0.45, ..frame }, radiusX: 18.0, radiusY: 18.0 };
        target.FillRoundedRectangle(&sheen, &brush([1.0; 3], 0.035)?);
        target.DrawRoundedRectangle(&shape, &brush([1.0; 3], 0.12)?, 1.0, None);
    }
    let ink = [0.95, 0.96, 0.98];
    let muted = [0.68, 0.72, 0.8];
    let (left, top) = (frame.left, frame.top);
    // The weather now: its sign, the temperature large, what it is and where on the right.
    write(sign(report.code, report.day), w!("Segoe UI Symbol"), 34.0, DWRITE_FONT_WEIGHT_NORMAL, D2D_RECT_F { left: left + 16.0, top: top + 12.0, right: left + 64.0, bottom: top + 70.0 }, DWRITE_TEXT_ALIGNMENT_CENTER, ink, 1.0)?;
    let degrees = format!("{}°", report.temperature.round() as i32);
    write(&degrees, w!("Segoe UI Variable Display"), 42.0, DWRITE_FONT_WEIGHT_LIGHT, D2D_RECT_F { left: left + 70.0, top: top + 8.0, right: left + 170.0, bottom: top + 74.0 }, DWRITE_TEXT_ALIGNMENT_LEADING, ink, 1.0)?;
    write(&report.condition, w!("Segoe UI Variable Text"), 13.5, DWRITE_FONT_WEIGHT_SEMI_BOLD, D2D_RECT_F { left: left + 150.0, top: top + 18.0, right: left + WIDTH - 18.0, bottom: top + 40.0 }, DWRITE_TEXT_ALIGNMENT_TRAILING, ink, 1.0)?;
    write(&report.place, w!("Segoe UI Variable Text"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, D2D_RECT_F { left: left + 120.0, top: top + 40.0, right: left + WIDTH - 18.0, bottom: top + 60.0 }, DWRITE_TEXT_ALIGNMENT_TRAILING, muted, 1.0)?;
    unsafe {
        let line = brush([1.0; 3], 0.1)?;
        target.DrawLine(Vector2 { X: left + 18.0, Y: top + 82.0 }, Vector2 { X: left + WIDTH - 18.0, Y: top + 82.0 }, &line, 1.0, None);
    }
    // The hours ahead.
    let count = report.hours.len().clamp(1, 6) as f32;
    let column = (WIDTH - 24.0) / count;
    for (i, &(hour, temperature, code)) in report.hours.iter().take(6).enumerate() {
        let x = left + 12.0 + i as f32 * column;
        let day = (7..19).contains(&hour);
        write(&format!("{hour:02}"), w!("Segoe UI Variable Text"), 11.0, DWRITE_FONT_WEIGHT_NORMAL, D2D_RECT_F { left: x, top: top + 88.0, right: x + column, bottom: top + 104.0 }, DWRITE_TEXT_ALIGNMENT_CENTER, muted, 1.0)?;
        write(sign(code as u32, day), w!("Segoe UI Symbol"), 15.0, DWRITE_FONT_WEIGHT_NORMAL, D2D_RECT_F { left: x, top: top + 104.0, right: x + column, bottom: top + 128.0 }, DWRITE_TEXT_ALIGNMENT_CENTER, ink, 0.95)?;
        write(&format!("{}°", temperature.round() as i32), w!("Segoe UI Variable Text"), 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, D2D_RECT_F { left: x, top: top + 128.0, right: x + column, bottom: top + 146.0 }, DWRITE_TEXT_ALIGNMENT_CENTER, ink, 1.0)?;
    }
    if report.hours.is_empty() {
        write(&t("No forecast yet"), w!("Segoe UI Variable Text"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, D2D_RECT_F { left: left + 18.0, top: top + 96.0, right: left + WIDTH - 18.0, bottom: top + 140.0 }, DWRITE_TEXT_ALIGNMENT_CENTER, muted, 1.0)?;
    }
    Ok(())
}

fn color(rgb: [f32; 3], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: rgb[0], g: rgb[1], b: rgb[2], a: alpha }
}
