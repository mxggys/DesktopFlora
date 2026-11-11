//! The settings window, laid out as Windows 11's own Settings: pages listed down the left, and on each page large
//! cards, one per setting, its name and what it does on the left and its switch, slider or choices on the right.
//! It sits on the system's Mica where Windows has it, and is drawn with Direct2D; everything is laid out again on each
//! paint, which also records where each control is, so a click finds its action.

use std::cell::{Cell, RefCell};
use std::mem::size_of;

use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dwm::{
    DWM_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyNameTextW, GetKeyState, MAPVK_VK_TO_VSC, MapVirtualKeyW, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK,
    VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, Result, w};
use windows_numerics::Vector2;

use snowhush_core::simulation::Mode;
use snowhush_core::text::{self as words, tr};

use crate::log;
use crate::prefs::{Hotkey, MAX_COLORS, SCHEDULES, SHORTCUTS, SIZES, Settings, WINDS};
use crate::text::{fill, t};
use crate::tracker::windows_build;

const CLASS: PCWSTR = w!("DesktopFloraSettings");
/// The window's client area, in DIPs.
const WIDTH: f32 = 900.0;
const HEIGHT: f32 = 660.0;
/// The navigation down the left, and the content's margins.
const NAV: f32 = 240.0;
const MARGIN: f32 = 36.0;
const CARD: f32 = 68.0;
const GAP: f32 = 4.0;
const WM_MOUSELEAVE: u32 = 0x02A3;
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
/// The first Windows 11 build with Mica for any window.
const MICA_BUILD: u32 = 22621;

/// Colours to pick for snow and stars, up to `MAX_COLORS` at once.
pub const COLOR_PRESETS: [(&str, [f32; 3]); 8] = [
    ("Ice Blue", [0.68, 0.84, 1.0]),
    ("Mint", [0.66, 1.0, 0.82]),
    ("Gold", [1.0, 0.84, 0.42]),
    ("Pink", [1.0, 0.68, 0.84]),
    ("Lavender", [0.8, 0.7, 1.0]),
    ("Red", [1.0, 0.42, 0.4]),
    ("Orange", [1.0, 0.64, 0.3]),
    ("Silver", [0.82, 0.84, 0.88]),
];

/// The pages, with their signs down the navigation.
const PAGES: [(&str, &str); 5] = [("❄", "Overview"), ("✿", "Look"), ("⚙", "Behavior"), ("⌨", "Programs & Shortcuts"), ("ⓘ", "About")];
/// How far one notch of the mouse wheel scrolls a page.
const WHEEL_STEP: f32 = 56.0;

/// What a click in the window asks for.
#[derive(Clone, Debug)]
pub enum Action {
    Toggle,
    /// How much falls, in percent, in steps of 5.
    Intensity(u32),
    /// How opaque what falls is drawn, in percent, 10 to 100.
    Opacity(u32),
    /// Picks what falls; mixed, it joins the other kind picked.
    Pick(Mode),
    Mixed,
    Halloween,
    /// Where the weather comes from: chosen here, the season, or the sky outside.
    Source(Source),
    Size(usize),
    Wind(usize),
    CustomColors(bool),
    /// Adds or takes away one of `COLOR_PRESETS`.
    Color(usize),
    Schedule(usize),
    /// Eco, normal, or max performance.
    Rate(usize),
    Ground,
    SnowOnWindows,
    Melt,
    DeepInteraction,
    BehindWindows,
    OverFullScreen,
    BehindTaskbar,
    Screenshots,
    FadeWhileTyping,
    OpenAtLogin,
    Clear,
    /// Keeps the snow off the program last in front.
    Exclude,
    /// Lets the snow back on the program at this place in the list.
    Include(usize),
    SetHotkey(usize, Option<Hotkey>),
    /// While a shortcut is being typed, the current ones let go of their keys, so typing one doesn't set it off.
    SuspendShortcuts(bool),
    Updates,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Chosen,
    Season,
    Outside,
}

/// What the window shows, read from the app each time it is drawn.
pub struct View {
    pub on: bool,
    pub settings: Settings,
    /// What falls now, whichever way it was chosen, and how much, in percent.
    pub falling: Vec<Mode>,
    pub intensity: u32,
    /// Two lines on where the weather comes from, as the season, the sky outside and the place.
    pub status: (String, String),
    /// Whether Halloween can be had now: only over snow alone.
    pub halloween_possible: bool,
    pub halloween_on: bool,
    pub excluded: Vec<String>,
    pub last_app: Option<String>,
    pub login: bool,
    /// Shortcuts another program already holds.
    pub taken: [bool; 3],
}

pub struct Handlers {
    pub view: Box<dyn Fn() -> Option<View>>,
    pub act: Box<dyn Fn(Action)>,
}

/// A slider's value range, in percent and steps of 5.
#[derive(Clone, Copy, Debug)]
enum Slider {
    Intensity,
    Opacity,
}

impl Slider {
    fn range(self) -> (u32, u32) {
        match self {
            Slider::Intensity => (0, 100),
            Slider::Opacity => (10, 100),
        }
    }

    fn action(self, value: u32) -> Action {
        match self {
            Slider::Intensity => Action::Intensity(value),
            Slider::Opacity => Action::Opacity(value),
        }
    }

    /// The value at `x` along `track`.
    fn at(self, track: &D2D_RECT_F, x: f32) -> u32 {
        let (low, high) = self.range();
        let t = ((x - track.left) / (track.right - track.left)).clamp(0.0, 1.0);
        let value = low as f32 + t * (high - low) as f32;
        ((value / 5.0).round() as u32 * 5).clamp(low, high)
    }
}

/// What the window does with a click: the app's actions, and its own pages, sliders and shortcut fields.
#[derive(Clone, Debug)]
enum Hit {
    App(Action),
    Page(usize),
    Capture(usize),
    Slider(Slider),
}

struct Window {
    window: HWND,
    scale: f32,
    mica: bool,
    page: usize,
    hover: Option<usize>,
    hits: Vec<(D2D_RECT_F, Hit)>,
    /// The shortcut waiting for its new keys.
    capturing: Option<usize>,
    /// The slider being dragged, its track, and the value last sent.
    dragging: Option<(Slider, D2D_RECT_F, u32)>,
    /// How far the page is scrolled down, and how far it can be.
    scroll: f32,
    scroll_limit: f32,
    handlers: Handlers,
    d2d: ID2D1Factory,
    text: IDWriteFactory,
    target: Option<ID2D1HwndRenderTarget>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    /// The page to open on next time.
    static LAST_PAGE: Cell<usize> = const { Cell::new(0) };
}

/// Opens the settings window, or brings it to the front if it is open.
pub fn show(handlers: Handlers) {
    if let Some(window) = WINDOW.with_borrow(|open| open.as_ref().map(|open| open.window)) {
        unsafe {
            let _ = ShowWindow(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    if let Err(error) = open(handlers) {
        log!("settings window failed: {error}");
    }
}

/// Draws the window again, if it is open, after something changed from elsewhere, as the tray menu or a shortcut.
pub fn refresh() {
    let window = WINDOW.with_borrow(|open| open.as_ref().map(|open| open.window));
    if let Some(window) = window {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(window), None, false);
        }
    }
}

pub fn close() {
    let window = WINDOW.with_borrow(|open| open.as_ref().map(|open| open.window));
    if let Some(window) = window {
        unsafe {
            let _ = DestroyWindow(window);
        }
    }
}

fn open(handlers: Handlers) -> Result<()> {
    let (scale, origin, outer) = placement();
    let dark = !light_theme();
    let mica = windows_build() >= MICA_BUILD;
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let icon = LoadImageW(Some(instance.into()), PCWSTR(1 as *const u16), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE).ok();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: icon.map(|icon| HICON(icon.0)).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let window = CreateWindowExW(
            WS_EX_APPWINDOW,
            CLASS,
            w!("DesktopFlora"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            origin.x,
            origin.y,
            outer.0,
            outer.1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let attribute = |kind, value: u32| {
            let _ = DwmSetWindowAttribute(window, kind, (&raw const value).cast(), 4);
        };
        attribute(DWMWA_USE_IMMERSIVE_DARK_MODE, BOOL::from(dark).0 as u32);
        if mica {
            // Mica shows wherever the window draws less than opaque: the whole client area is frame to the system.
            let _ = DwmExtendFrameIntoClientArea(window, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
            attribute(DWMWA_SYSTEMBACKDROP_TYPE, DWM_SYSTEMBACKDROP_TYPE(DWMSBT_MAINWINDOW.0).0 as u32);
        }
        WINDOW.set(Some(Window {
            window,
            scale,
            mica,
            page: LAST_PAGE.get(),
            hover: None,
            hits: Vec::new(),
            capturing: None,
            dragging: None,
            scroll: 0.0,
            scroll_limit: 0.0,
            handlers,
            d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            text: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
            target: None,
        }));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
    Ok(())
}

/// The display's scale, where the window goes (centred on the display under the pointer) and its outer size, title bar
/// and borders included, for a client area of `WIDTH` by `HEIGHT`.
fn placement() -> (f32, POINT, (i32, i32)) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi, mut _y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y);
        let scale = dpi.max(96) as f32 / 96.0;
        let mut frame = RECT { left: 0, top: 0, right: (WIDTH * scale).round() as i32, bottom: (HEIGHT * scale).round() as i32 };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
            &mut frame,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            false,
            WS_EX_APPWINDOW,
            dpi,
        );
        let outer = (frame.right - frame.left, frame.bottom - frame.top);
        let RECT { left, top, right, bottom } = info.rcWork;
        let x = left + ((right - left - outer.0) / 2).max(0);
        let y = top + ((bottom - top - outer.1) / 2).max(0);
        (scale, POINT { x, y }, outer)
    }
}

fn paint() {
    let result = WINDOW.with_borrow_mut(|open| {
        let open = open.as_mut()?;
        let view = (open.handlers.view)()?;
        Some(open.render(&view))
    });
    if let Some(Err(error)) = result {
        log!("settings paint failed: {error}");
        // A lost device takes the render target with it; the next paint makes a new one.
        WINDOW.with_borrow_mut(|open| {
            if let Some(open) = open {
                open.target = None;
            }
        });
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            paint();
            unsafe {
                let _ = ValidateRect(Some(window), None);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEWHEEL => {
            let notches = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
            let moved = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                let scroll = (open.scroll - notches * WHEEL_STEP).clamp(0.0, open.scroll_limit);
                let moved = scroll != open.scroll;
                open.scroll = scroll;
                Some(moved)
            });
            if moved == Some(true) {
                paint();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(open) = WINDOW.take() {
                LAST_PAGE.set(open.page);
                if open.capturing.is_some() {
                    (open.handlers.act)(Action::SuspendShortcuts(false));
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let point = point(lparam);
            // Dragging a slider, its value follows the pointer.
            let dragged = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                let (slider, track, last) = open.dragging?;
                let value = slider.at(&track, point.x as f32 / open.scale);
                (value != last).then(|| {
                    open.dragging = Some((slider, track, value));
                    slider.action(value)
                })
            });
            if let Some(action) = dragged {
                click(Hit::App(action));
                return LRESULT(0);
            }
            let changed = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                let at = (point.x as f32 / open.scale, point.y as f32 / open.scale);
                let hover = open.hits.iter().position(|(rect, _)| contains(rect, at));
                let changed = hover != open.hover;
                open.hover = hover;
                Some(changed)
            });
            if changed == Some(true) {
                let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: window, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut track);
                }
                paint();
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            WINDOW.with_borrow_mut(|open| {
                if let Some(open) = open {
                    open.hover = None;
                }
            });
            paint();
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = point(lparam);
            let pressed = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                let at = (point.x as f32 / open.scale, point.y as f32 / open.scale);
                let (track, slider) = open.hits.iter().find_map(|(rect, hit)| match hit {
                    Hit::Slider(slider) if contains(rect, at) => Some((*rect, *slider)),
                    _ => None,
                })?;
                let value = slider.at(&track, at.0);
                open.dragging = Some((slider, track, value));
                Some(slider.action(value))
            });
            if let Some(action) = pressed {
                unsafe { SetCapture(window) };
                click(Hit::App(action));
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let point = point(lparam);
            let was_dragging = WINDOW.with_borrow_mut(|open| open.as_mut().and_then(|open| open.dragging.take()).is_some());
            if was_dragging {
                unsafe {
                    let _ = ReleaseCapture();
                }
                return LRESULT(0);
            }
            let hit = WINDOW.with_borrow(|open| {
                let open = open.as_ref()?;
                let at = (point.x as f32 / open.scale, point.y as f32 / open.scale);
                open.hits.iter().find(|(rect, _)| contains(rect, at)).map(|(_, hit)| hit.clone())
            });
            if let Some(hit) = hit {
                click(hit);
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if key(wparam.0 as u32) {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn point(lparam: LPARAM) -> POINT {
    POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 }
}

fn contains(rect: &D2D_RECT_F, (x, y): (f32, f32)) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn click(hit: Hit) {
    match hit {
        Hit::Slider(_) => {}
        Hit::Page(page) => {
            let was_capturing = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                open.page = page;
                open.scroll = 0.0;
                open.capturing.take()
            });
            if was_capturing.is_some() {
                suspend(false);
            }
            paint();
        }
        Hit::Capture(index) => {
            let capturing = WINDOW.with_borrow_mut(|open| {
                let open = open.as_mut()?;
                open.capturing = if open.capturing == Some(index) { None } else { Some(index) };
                Some(open.capturing.is_some())
            });
            if let Some(capturing) = capturing {
                suspend(capturing);
            }
            paint();
        }
        Hit::App(Action::Quit) => {
            let act = take_act();
            close();
            if let Some(act) = act {
                act(Action::Quit);
            }
        }
        Hit::App(action) => {
            // The handler may open a message box: act outside the borrow.
            let act = take_act();
            if let Some(act) = &act {
                act(action);
            }
            put_act(act);
            paint();
        }
    }
}

/// Takes the action handler out of the window while it runs, so the window can be borrowed, or closed, meanwhile.
fn take_act() -> Option<Box<dyn Fn(Action)>> {
    WINDOW.with_borrow_mut(|open| open.as_mut().map(|open| std::mem::replace(&mut open.handlers.act, Box::new(|_| {}))))
}

fn put_act(act: Option<Box<dyn Fn(Action)>>) {
    let Some(act) = act else { return };
    WINDOW.with_borrow_mut(|open| {
        if let Some(open) = open {
            open.handlers.act = act;
        }
    });
}

fn suspend(on: bool) {
    let act = take_act();
    if let Some(act) = &act {
        act(Action::SuspendShortcuts(on));
    }
    put_act(act);
}

/// A key pressed in the window: while a shortcut waits for its keys, they become it. Whether the key was taken.
fn key(code: u32) -> bool {
    let capturing = WINDOW.with_borrow(|open| open.as_ref().and_then(|open| open.capturing));
    let Some(index) = capturing else {
        if code == VK_ESCAPE.0 as u32 {
            close();
            return true;
        }
        return false;
    };
    let modifiers_only = [VK_CONTROL.0, VK_MENU.0, VK_SHIFT.0, VK_LWIN.0, VK_RWIN.0, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5].contains(&(code as u16));
    if modifiers_only {
        return true;
    }
    let hotkey = if code == VK_ESCAPE.0 as u32 {
        stop_capturing();
        return true;
    } else if code == VK_BACK.0 as u32 || code == VK_DELETE.0 as u32 {
        None
    } else {
        let down = |key: u16| unsafe { GetKeyState(key as i32) } < 0;
        let modifiers = if down(VK_CONTROL.0) { MOD_CONTROL } else { 0 }
            | if down(VK_MENU.0) { MOD_ALT } else { 0 }
            | if down(VK_SHIFT.0) { MOD_SHIFT } else { 0 }
            | if down(VK_LWIN.0) || down(VK_RWIN.0) { MOD_WIN } else { 0 };
        // A shortcut without Ctrl, Alt or Win would take the key away from typing.
        if modifiers & (MOD_CONTROL | MOD_ALT | MOD_WIN) == 0 {
            return true;
        }
        Some(Hotkey { modifiers, key: code })
    };
    stop_capturing();
    click(Hit::App(Action::SetHotkey(index, hotkey)));
    true
}

fn stop_capturing() {
    let was = WINDOW.with_borrow_mut(|open| open.as_mut().and_then(|open| open.capturing.take()));
    if was.is_some() {
        suspend(false);
    }
    paint();
}

/// A shortcut as people read it, as "Ctrl+Alt+S", with the modifier names in the person's language.
pub fn describe(hotkey: Hotkey) -> String {
    let mut text = String::new();
    for (flag, name) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if hotkey.modifiers & flag != 0 {
            text.push_str(&t(name));
            text.push('+');
        }
    }
    let key = hotkey.key;
    let name = match key {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(key).map(String::from).unwrap_or_default(),
        0x70..=0x87 => format!("F{}", key - 0x6F),
        _ => unsafe {
            let scan = MapVirtualKeyW(key, MAPVK_VK_TO_VSC);
            let mut buffer = [0u16; 32];
            let length = GetKeyNameTextW((scan << 16) as i32, &mut buffer);
            if length > 0 { String::from_utf16_lossy(&buffer[..length as usize]) } else { format!("#{key}") }
        },
    };
    text + &name
}

fn light_theme() -> bool {
    let mut light = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && light != 0
}

/// The window's colours, in Windows 11's Settings: cards a shade apart from the Mica, ink and an accent.
struct Palette {
    /// Over Mica, a light veil; without it, the whole background.
    base: [f32; 4],
    card: [f32; 4],
    ink: [f32; 3],
    muted: f32,
    accent: [f32; 3],
    on_accent: [f32; 3],
    warning: [f32; 3],
}

fn palette(mica: bool) -> Palette {
    if light_theme() {
        Palette {
            base: [0.95, 0.95, 0.96, if mica { 0.0 } else { 1.0 }],
            card: [1.0, 1.0, 1.0, 0.72],
            ink: [0.1, 0.1, 0.11],
            muted: 0.62,
            accent: [0.0, 0.37, 0.72],
            on_accent: [1.0; 3],
            warning: [0.77, 0.2, 0.16],
        }
    } else {
        Palette {
            base: [0.125, 0.125, 0.13, if mica { 0.0 } else { 1.0 }],
            card: [1.0, 1.0, 1.0, 0.055],
            ink: [1.0, 1.0, 1.0],
            muted: 0.62,
            accent: [0.38, 0.75, 1.0],
            on_accent: [0.0, 0.0, 0.0],
            warning: [1.0, 0.55, 0.5],
        }
    }
}

impl Window {
    fn render(&mut self, view: &View) -> Result<()> {
        if self.target.is_none() {
            let mut client = RECT::default();
            unsafe {
                let _ = GetClientRect(self.window, &mut client);
                let properties = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0 * self.scale,
                    dpiY: 96.0 * self.scale,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                };
                let window = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd: self.window,
                    pixelSize: D2D_SIZE_U { width: client.right as u32, height: client.bottom as u32 },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                };
                self.target = Some(self.d2d.CreateHwndRenderTarget(&properties, &window)?);
            }
        }
        let Some(target) = &self.target else { return Ok(()) };
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color([0.0; 3], 0.0)));
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        }
        let mut painter = Painter { target, text: &self.text, colors: palette(self.mica), hits: Vec::new(), hover: self.hover };
        let drawn = painter.window(view, self.page, self.capturing, self.scroll);
        self.hits = std::mem::take(&mut painter.hits);
        unsafe { target.EndDraw(None, None)? };
        // What the page holds below the window's bottom edge can be scrolled up into view.
        let bottom = drawn? + self.scroll;
        self.scroll_limit = (bottom + 24.0 - HEIGHT).max(0.0);
        self.scroll = self.scroll.min(self.scroll_limit);
        Ok(())
    }
}

/// Draws the window and records where its controls are.
struct Painter<'a> {
    target: &'a ID2D1RenderTarget,
    text: &'a IDWriteFactory,
    colors: Palette,
    hits: Vec<(D2D_RECT_F, Hit)>,
    hover: Option<usize>,
}

/// What falls, in the order the tiles show them, with their signs.
const KINDS: [(Mode, &str); 11] = [
    (Mode::Snow, "❄"),
    (Mode::Autumn, "❦"),
    (Mode::Rain, "☂"),
    (Mode::Blossom, "✿"),
    (Mode::Stars, "✦"),
    (Mode::Storm, "⚡"),
    (Mode::Fog, "≋"),
    (Mode::Confetti, "❖"),
    (Mode::Fireworks, "✺"),
    (Mode::Hail, "❅"),
    (Mode::Fireflies, "✧"),
];

/// A kind's name in the person's language.
fn name_of(mode: Mode) -> String {
    words::mode_name(mode).to_string()
}

/// The control on the right of a card.
enum Control<'a> {
    Switch(bool, Action),
    Choices(Vec<String>, Option<usize>, &'a dyn Fn(usize) -> Action),
    Slider(String, u32, Slider),
    Button(String, Hit),
    Keys(String, [f32; 3], bool, usize),
    Remove(usize),
}

impl Painter<'_> {
    /// Draws the window, its page scrolled up by `scroll`; returns where the page's content ends.
    fn window(&mut self, view: &View, page: usize, capturing: Option<usize>, scroll: f32) -> Result<f32> {
        let frame = D2D_RECT_F { left: 0.0, top: 0.0, right: WIDTH, bottom: HEIGHT };
        let [r, g, b, a] = self.colors.base;
        self.fill(frame, 0.0, [r, g, b], a)?;

        // The navigation: the app's name and whether it snows, then the pages.
        self.label("DesktopFlora", D2D_RECT_F { left: 24.0, top: 22.0, right: NAV - 70.0, bottom: 48.0 }, 18.0, true, self.colors.ink, 1.0, LEADING)?;
        let state = if view.on {
            let names: Vec<String> = view.falling.iter().map(|&mode| name_of(mode)).collect();
            let amount = if view.intensity == 0 { tr("off").to_string() } else { format!("{}%", view.intensity) };
            format!("{} · {}", names.join(" + "), amount)
        } else {
            t("Paused")
        };
        self.label(&state, D2D_RECT_F { left: 24.0, top: 48.0, right: NAV - 70.0, bottom: 68.0 }, 12.0, false, self.colors.ink, self.colors.muted, LEADING)?;
        self.switch(D2D_RECT_F { left: NAV - 64.0, top: 34.0, right: NAV - 20.0, bottom: 56.0 }, view.on, true, Hit::App(Action::Toggle))?;
        for (i, (sign, name)) in PAGES.iter().enumerate() {
            let top = 96.0 + i as f32 * 44.0;
            let rect = D2D_RECT_F { left: 12.0, top, right: NAV - 12.0, bottom: top + 40.0 };
            let chosen = i == page;
            if chosen {
                self.fill(rect, 6.0, self.colors.ink, 0.08)?;
                // Windows 11's pill marks the page shown.
                self.fill(D2D_RECT_F { left: rect.left, top: rect.top + 11.0, right: rect.left + 3.0, bottom: rect.bottom - 11.0 }, 1.5, self.colors.accent, 1.0)?;
            }
            self.hit(rect, Hit::Page(i), 6.0)?;
            self.glyph(sign, D2D_RECT_F { left: rect.left + 12.0, right: rect.left + 40.0, ..rect }, 15.0, self.colors.ink)?;
            self.label(&t(name), D2D_RECT_F { left: rect.left + 48.0, ..rect }, 13.5, chosen, self.colors.ink, 1.0, LEADING)?;
        }
        let quit = D2D_RECT_F { left: 12.0, top: HEIGHT - 56.0, right: NAV - 12.0, bottom: HEIGHT - 16.0 };
        self.hit(quit, Hit::App(Action::Quit), 6.0)?;
        self.glyph("⏻", D2D_RECT_F { left: quit.left + 12.0, right: quit.left + 40.0, ..quit }, 15.0, self.colors.ink)?;
        self.label(tr("quit"), D2D_RECT_F { left: quit.left + 48.0, ..quit }, 13.5, false, self.colors.ink, 1.0, LEADING)?;

        // The page: its title, then its cards, scrolled and kept to the right of the navigation.
        let (left, right) = (NAV + MARGIN - 12.0, WIDTH - MARGIN);
        let clip = D2D_RECT_F { left: NAV, top: 0.0, right: WIDTH, bottom: HEIGHT };
        unsafe { self.target.PushAxisAlignedClip(&clip, windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED) };
        let first = self.hits.len();
        let title = PAGES.get(page).map_or("", |(_, name)| name);
        self.label(&t(title), D2D_RECT_F { left, top: 20.0 - scroll, right, bottom: 64.0 - scroll }, 26.0, true, self.colors.ink, 1.0, LEADING)?;
        let mut y = 84.0 - scroll;
        let drawn = match page {
            0 => self.overview(view, left, right, &mut y),
            1 => self.look(view, left, right, &mut y),
            2 => self.behavior(view, left, right, &mut y),
            3 => self.programs(view, left, right, &mut y, capturing),
            _ => self.about(left, right, &mut y),
        };
        unsafe { self.target.PopAxisAlignedClip() };
        // Controls scrolled out of sight can't be clicked.
        let mut i = first;
        while i < self.hits.len() {
            let (rect, _) = &self.hits[i];
            if rect.bottom <= 0.0 || rect.top >= HEIGHT {
                self.hits.remove(i);
            } else {
                i += 1;
            }
        }
        drawn?;
        // A thin bar on the right says how far down the page is, when there is more of it than fits.
        let content = y + scroll;
        if content > HEIGHT - 24.0 {
            let share = (HEIGHT / (content + 24.0)).min(1.0);
            let length = (HEIGHT - 16.0) * share;
            let along = (scroll / (content + 24.0 - HEIGHT).max(1.0)).clamp(0.0, 1.0) * (HEIGHT - 16.0 - length);
            self.fill(D2D_RECT_F { left: WIDTH - 7.0, top: 8.0 + along, right: WIDTH - 4.0, bottom: 8.0 + along + length }, 1.5, self.colors.ink, 0.35)?;
        }
        Ok(y)
    }

    /// A heading over a group of cards.
    fn section(&mut self, text: &str, left: f32, right: f32, y: &mut f32) -> Result<()> {
        *y += 10.0;
        self.label(text, D2D_RECT_F { left: left + 2.0, top: *y, right, bottom: *y + 22.0 }, 14.0, true, self.colors.ink, 1.0, LEADING)?;
        *y += 30.0;
        Ok(())
    }

    fn overview(&mut self, view: &View, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let settings = &view.settings;
        let source = if settings.local_weather {
            Source::Outside
        } else if settings.seasonal {
            Source::Season
        } else {
            Source::Chosen
        };
        let sources = [Source::Chosen, Source::Season, Source::Outside];
        let names: Vec<String> = ["Choose", "Season", "Outside"].iter().map(|name| t(name)).collect();
        let pick = |i: usize| Action::Source(sources[i]);
        let chosen = sources.iter().position(|&s| s == source);
        self.card(&t("What falls"), &t("Choose it yourself, follow the season where you live, or the weather outside"), Control::Choices(names, chosen, &pick), left, right, y)?;

        if source == Source::Chosen {
            // A big tile per kind, its sign over its name, six to a row; mixed, two can be picked.
            let columns = 6.0;
            let width = (right - left - (columns - 1.0) * 8.0) / columns;
            let picked = settings.modes();
            for (i, (mode, sign)) in KINDS.iter().enumerate() {
                let x = left + (i % 6) as f32 * (width + 8.0);
                let top = *y + (i / 6) as f32 * (92.0 + 8.0);
                let rect = D2D_RECT_F { left: x, top, right: x + width, bottom: top + 92.0 };
                let selected = picked.contains(mode);
                if selected {
                    self.fill(rect, 8.0, self.colors.accent, 1.0)?;
                } else {
                    let [r, g, b, a] = self.colors.card;
                    self.fill(rect, 8.0, [r, g, b], a)?;
                }
                self.hit(rect, Hit::App(Action::Pick(*mode)), 8.0)?;
                let ink = if selected { self.colors.on_accent } else { self.colors.ink };
                self.glyph(sign, D2D_RECT_F { top: rect.top + 12.0, bottom: rect.top + 54.0, ..rect }, 26.0, ink)?;
                self.label(&name_of(*mode), D2D_RECT_F { left: rect.left + 4.0, right: rect.right - 4.0, top: rect.top + 58.0, bottom: rect.bottom - 10.0 }, 11.5, selected, ink, 1.0, CENTER)?;
            }
            let rows = KINDS.len().div_ceil(6) as f32;
            *y += rows * 100.0 + GAP + 4.0;
            self.card(&t("Mix two"), &t("Two kinds fall together"), Control::Switch(settings.mixed, Action::Mixed), left, right, y)?;
            let note = if view.halloween_possible { t("Jack-o'-lanterns and ghosts in the snow") } else { t("Only over snow alone") };
            if view.halloween_possible {
                self.card(tr("halloween"), &note, Control::Switch(view.halloween_on, Action::Halloween), left, right, y)?;
            } else {
                self.note_card(tr("halloween"), &note, left, right, y)?;
            }
        } else {
            self.note_card(&view.status.0, &view.status.1, left, right, y)?;
        }

        let amount = if view.intensity == 0 { tr("off").to_string() } else { format!("{}%", view.intensity) };
        if source == Source::Outside {
            self.note_card(tr("intensity"), &fill(&t("As heavy as outside: {}"), &[&amount]), left, right, y)?;
        } else {
            self.card(tr("intensity"), &t("How much falls"), Control::Slider(amount, view.intensity, Slider::Intensity), left, right, y)?;
        }
        // Rain and stars don't lie anywhere: nothing to clear.
        let heaps: Vec<Mode> = view.falling.iter().copied().filter(|mode| mode.heaps()).collect();
        if !heaps.is_empty() {
            let clear = words::kinds("clear", &heaps).to_string();
            self.card(&clear, &t("Blows away everything lying on the screen"), Control::Button(clear.clone(), Hit::App(Action::Clear)), left, right, y)?;
        }
        Ok(())
    }

    fn look(&mut self, view: &View, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let settings = &view.settings;
        let names = |options: &[(&str, f32)]| options.iter().map(|(name, _)| t(name)).collect::<Vec<_>>();
        self.card(&t("Size"), &t("How big flakes and stars are drawn"), Control::Choices(names(&SIZES), Some(settings.size), &Action::Size), left, right, y)?;
        self.card(&t("Wind"), &t("How hard the breeze blows"), Control::Choices(names(&WINDS), Some(settings.wind), &Action::Wind), left, right, y)?;
        let opacity = format!("{}%", settings.opacity);
        self.card(tr("opacity"), &t("How see-through what falls is"), Control::Slider(opacity, settings.opacity, Slider::Opacity), left, right, y)?;
        let kinds = vec![t("Natural"), t("My Colors")];
        let custom = |i: usize| Action::CustomColors(i == 1);
        self.card(&t("Color"), &t("Snow and stars take your colors; leaves, petals and rain keep their own"), Control::Choices(kinds, Some(settings.custom_colors as usize), &custom), left, right, y)?;
        if settings.custom_colors {
            // The colours to pick from, as big round swatches; up to five at once.
            let card = self.card_frame(left, right, *y, 96.0)?;
            let palette = &settings.palette[..settings.palette_len.min(MAX_COLORS)];
            let diameter = 40.0;
            let gap = ((card.right - card.left - 40.0) - COLOR_PRESETS.len() as f32 * diameter) / (COLOR_PRESETS.len() as f32 - 1.0);
            for (i, (name, rgb)) in COLOR_PRESETS.iter().enumerate() {
                let x = card.left + 20.0 + i as f32 * (diameter + gap);
                let swatch = D2D_RECT_F { left: x, top: card.top + 14.0, right: x + diameter, bottom: card.top + 14.0 + diameter };
                let on = palette.contains(rgb);
                if on {
                    let ring = D2D_RECT_F { left: x - 4.0, top: swatch.top - 4.0, right: swatch.right + 4.0, bottom: swatch.bottom + 4.0 };
                    self.outline(ring, diameter, self.colors.accent, 1.0, 2.0)?;
                }
                self.fill(swatch, diameter / 2.0, *rgb, 1.0)?;
                if on || palette.len() < MAX_COLORS {
                    self.hit(swatch, Hit::App(Action::Color(i)), diameter / 2.0)?;
                }
                let label = D2D_RECT_F { left: x - 14.0, top: swatch.bottom + 6.0, right: swatch.right + 14.0, bottom: swatch.bottom + 24.0 };
                self.label(&t(name), label, 11.0, on, self.colors.ink, if on { 1.0 } else { self.colors.muted }, CENTER)?;
            }
            *y += 96.0 + GAP;
        }
        Ok(())
    }

    fn behavior(&mut self, view: &View, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let settings = &view.settings;
        let heaps: Vec<Mode> = view.falling.iter().copied().filter(|mode| mode.heaps()).collect();
        // Only what heaps up melts, withers or wilts, each by its own name.
        let melt = tr(if heaps.contains(&Mode::Snow) {
            "melt.snow"
        } else if heaps == [Mode::Blossom] {
            "melt.blossom"
        } else {
            "melt.autumn"
        });
        self.section(tr("accumulate"), left, right, y)?;
        self.card(tr("surface.ground"), &t("Drifts along the bottom of the screen"), Control::Switch(settings.ground, Action::Ground), left, right, y)?;
        self.card(tr("surface.windows"), &t("Settles on the tops of windows and slides off when you drag them"), Control::Switch(settings.snow_on_windows, Action::SnowOnWindows), left, right, y)?;
        if !heaps.is_empty() {
            self.card(melt, &t("What lies thaws slowly, so it never takes over the screen"), Control::Switch(settings.melt, Action::Melt), left, right, y)?;
        }
        self.card(tr("deep"), &t("Also settles on a selection dragged across the desktop"), Control::Switch(settings.deep_interaction, Action::DeepInteraction), left, right, y)?;

        self.section(tr("display"), left, right, y)?;
        self.card(tr("behind"), &t("Falls behind your windows, in front of the wallpaper"), Control::Switch(settings.behind_windows, Action::BehindWindows), left, right, y)?;
        if !settings.behind_windows {
            self.card(tr("full_screen"), &t("Stays over full-screen apps and games"), Control::Switch(settings.over_full_screen, Action::OverFullScreen), left, right, y)?;
        }
        self.card(tr("taskbar"), &t("Settles under a see-through taskbar, as with TranslucentTB"), Control::Switch(settings.behind_taskbar, Action::BehindTaskbar), left, right, y)?;
        self.card(tr("screenshots"), &t("Screenshots and screen sharing show it too"), Control::Switch(settings.show_in_screenshots, Action::Screenshots), left, right, y)?;
        let rates = vec![t("Eco"), t("Normal"), t("Max")];
        let rate = if settings.max_performance {
            2
        } else if settings.eco {
            0
        } else {
            1
        };
        self.card(&t("Frame Rate"), &t("Eco: at most 20 frames a second. Max: always the display's full rate"), Control::Choices(rates, Some(rate), &Action::Rate), left, right, y)?;

        self.section(tr("general"), left, right, y)?;
        self.card(tr("login"), &t("Starts with Windows"), Control::Switch(view.login, Action::OpenAtLogin), left, right, y)?;
        self.card(&t("Fade While Typing"), &t("Turns faint while you type"), Control::Switch(settings.fade_while_typing, Action::FadeWhileTyping), left, right, y)?;
        let schedules: Vec<String> = SCHEDULES.iter().map(|(name, _, _)| t(name)).collect();
        self.card(&t("Schedule"), &t("When it shows"), Control::Choices(schedules, Some(settings.schedule), &Action::Schedule), left, right, y)
    }

    /// Snow-free programs and keyboard shortcuts, under their headings.
    fn programs(&mut self, view: &View, left: f32, right: f32, y: &mut f32, capturing: Option<usize>) -> Result<()> {
        self.section(&t("Snow-Free Programs"), left, right, y)?;
        self.snow_free(view, left, right, y)?;
        self.section(&t("Keyboard Shortcuts"), left, right, y)?;
        self.shortcuts(view, left, right, y, capturing)
    }

    fn snow_free(&mut self, view: &View, left: f32, right: f32, y: &mut f32) -> Result<()> {
        match &view.last_app {
            Some(app) => {
                let add = fill(&t("Keep Snow Off {}"), &[app]);
                self.card(&add, &t("No snow on its windows; maximized in front, none on that screen at all"), Control::Button(t("Add"), Hit::App(Action::Exclude)), left, right, y)?;
            }
            None => self.note_card(&t("Add a program"), &t("Switch to the program, then open the settings again"), left, right, y)?,
        }
        if view.excluded.is_empty() {
            return self.note_card(&t("No programs yet"), &t("Snow falls on every window"), left, right, y);
        }
        for (i, app) in view.excluded.iter().enumerate().take(6) {
            self.card(app, &t("Snow-free"), Control::Remove(i), left, right, y)?;
        }
        Ok(())
    }

    fn shortcuts(&mut self, view: &View, left: f32, right: f32, y: &mut f32, capturing: Option<usize>) -> Result<()> {
        for (i, (name, _, _)) in SHORTCUTS.iter().enumerate() {
            let (text, ink, waiting) = if capturing == Some(i) {
                (t("Press keys…"), self.colors.accent, true)
            } else if let Some(hotkey) = view.settings.hotkeys[i] {
                let text = describe(hotkey);
                if view.taken[i] { (fill(&t("{} (taken)"), &[&text]), self.colors.warning, false) } else { (text, self.colors.ink, false) }
            } else {
                (t("None"), self.colors.ink, false)
            };
            self.card(&t(name), &t("Click, then press the new keys; Delete removes it"), Control::Keys(text, ink, waiting, i), left, right, y)?;
        }
        self.note_card(&t("Double-click the tray icon"), &t("Switches the snow on and off"), left, right, y)
    }

    fn about(&mut self, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let version = format!("DesktopFlora {}", env!("CARGO_PKG_VERSION"));
        self.card(&version, &t("Updates install themselves, checked against the release's checksum"), Control::Button(tr("updates").to_string(), Hit::App(Action::Updates)), left, right, y)?;
        self.note_card(&t("Based on Snowhush"), &t("By Mykhailo Bezruchko, under the PolyForm Noncommercial license"), left, right, y)
    }

    /// A card: a name and what it does on the left, its control on the right.
    fn card(&mut self, title: &str, detail: &str, control: Control, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let card = self.card_frame(left, right, *y, CARD)?;
        let control_width = match &control {
            Control::Switch(..) => 120.0,
            Control::Choices(options, ..) => (options.len() as f32 * 86.0).min(360.0),
            Control::Slider(..) => 300.0,
            Control::Button(..) | Control::Keys(..) => 180.0,
            Control::Remove(_) => 120.0,
        };
        let text_right = card.right - control_width - 24.0;
        self.label(title, D2D_RECT_F { left: card.left + 20.0, top: card.top + 12.0, right: text_right, bottom: card.top + 36.0 }, 14.0, false, self.colors.ink, 1.0, LEADING)?;
        self.label(detail, D2D_RECT_F { left: card.left + 20.0, top: card.top + 34.0, right: text_right, bottom: card.top + 56.0 }, 12.0, false, self.colors.ink, self.colors.muted, LEADING)?;
        let area = D2D_RECT_F { left: card.right - control_width - 16.0, top: card.top, right: card.right - 16.0, bottom: card.bottom };
        let middle = (card.top + card.bottom) / 2.0;
        match control {
            Control::Switch(on, action) => {
                let label = if on { t("On") } else { t("Off") };
                self.label(&label, D2D_RECT_F { right: area.right - 54.0, ..area }, 13.0, false, self.colors.ink, 1.0, TRAILING)?;
                let switch = D2D_RECT_F { left: area.right - 44.0, top: middle - 10.0, right: area.right, bottom: middle + 10.0 };
                // The whole card switches it: a big surface to click.
                self.hit(card, Hit::App(action), 6.0)?;
                self.switch_shape(switch, on)?;
            }
            Control::Choices(options, chosen, action) => {
                let bar = D2D_RECT_F { left: area.left, top: middle - 16.0, right: area.right, bottom: middle + 16.0 };
                self.fill(bar, 6.0, self.colors.ink, 0.06)?;
                let width = (bar.right - bar.left - 4.0) / options.len() as f32;
                for (i, option) in options.iter().enumerate() {
                    let x = bar.left + 2.0 + i as f32 * width;
                    let rect = D2D_RECT_F { left: x, top: bar.top + 2.0, right: x + width, bottom: bar.bottom - 2.0 };
                    let selected = chosen == Some(i);
                    if selected {
                        self.fill(rect, 5.0, self.colors.accent, 1.0)?;
                    }
                    self.hit(rect, Hit::App(action(i)), 5.0)?;
                    let ink = if selected { self.colors.on_accent } else { self.colors.ink };
                    self.label(option, rect, 12.5, selected, ink, 1.0, CENTER)?;
                }
            }
            Control::Slider(text, value, slider) => {
                self.label(&text, D2D_RECT_F { left: area.right - 52.0, ..area }, 13.0, false, self.colors.ink, 1.0, TRAILING)?;
                let (low, high) = slider.range();
                let share = (value.clamp(low, high) - low) as f32 / (high - low) as f32;
                let track = D2D_RECT_F { left: area.left + 8.0, top: middle - 2.0, right: area.right - 64.0, bottom: middle + 2.0 };
                self.fill(track, 2.0, self.colors.ink, 0.25)?;
                let x = track.left + share * (track.right - track.left);
                self.fill(D2D_RECT_F { right: x, ..track }, 2.0, self.colors.accent, 1.0)?;
                unsafe {
                    let ring = self.target.CreateSolidColorBrush(&color(self.colors.ink, 0.9), None)?;
                    let dot = self.target.CreateSolidColorBrush(&color(self.colors.accent, 1.0), None)?;
                    self.target.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: middle }, radiusX: 10.0, radiusY: 10.0 }, &ring);
                    self.target.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: middle }, radiusX: 6.0, radiusY: 6.0 }, &dot);
                }
                // The whole track's height takes a press, so the knob needn't be hit exactly.
                let press = D2D_RECT_F { left: track.left, top: card.top + 8.0, right: track.right, bottom: card.bottom - 8.0 };
                self.hits.push((press, Hit::Slider(slider)));
            }
            Control::Button(text, hit) => {
                let button = D2D_RECT_F { left: area.left, top: middle - 16.0, right: area.right, bottom: middle + 16.0 };
                self.fill(button, 5.0, self.colors.ink, 0.08)?;
                self.outline(button, 5.0, self.colors.ink, 0.12, 1.0)?;
                self.hit(button, hit, 5.0)?;
                self.label(&text, button, 12.5, false, self.colors.ink, 1.0, CENTER)?;
            }
            Control::Keys(text, ink, waiting, index) => {
                let field = D2D_RECT_F { left: area.left, top: middle - 16.0, right: area.right, bottom: middle + 16.0 };
                self.fill(field, 5.0, self.colors.ink, 0.08)?;
                self.outline(field, 5.0, if waiting { self.colors.accent } else { self.colors.ink }, if waiting { 1.0 } else { 0.12 }, if waiting { 2.0 } else { 1.0 })?;
                self.hit(field, Hit::Capture(index), 5.0)?;
                self.label(&text, field, 12.5, false, ink, 1.0, CENTER)?;
            }
            Control::Remove(index) => {
                let button = D2D_RECT_F { left: area.left, top: middle - 16.0, right: area.right, bottom: middle + 16.0 };
                self.fill(button, 5.0, self.colors.ink, 0.08)?;
                self.hit(button, Hit::App(Action::Include(index)), 5.0)?;
                self.label(&t("Remove"), button, 12.5, false, self.colors.ink, 1.0, CENTER)?;
            }
        }
        *y += CARD + GAP;
        Ok(())
    }

    /// A card that only says something.
    fn note_card(&mut self, title: &str, detail: &str, left: f32, right: f32, y: &mut f32) -> Result<()> {
        let card = self.card_frame(left, right, *y, CARD)?;
        self.label(title, D2D_RECT_F { left: card.left + 20.0, top: card.top + 12.0, right: card.right - 20.0, bottom: card.top + 36.0 }, 14.0, false, self.colors.ink, 1.0, LEADING)?;
        self.label(detail, D2D_RECT_F { left: card.left + 20.0, top: card.top + 34.0, right: card.right - 20.0, bottom: card.top + 56.0 }, 12.0, false, self.colors.ink, self.colors.muted, LEADING)?;
        *y += CARD + GAP;
        Ok(())
    }

    fn card_frame(&self, left: f32, right: f32, top: f32, height: f32) -> Result<D2D_RECT_F> {
        let card = D2D_RECT_F { left, top, right, bottom: top + height };
        let [r, g, b, a] = self.colors.card;
        self.fill(card, 6.0, [r, g, b], a)?;
        self.outline(card, 6.0, self.colors.ink, 0.06, 1.0)?;
        Ok(card)
    }

    fn switch(&mut self, rect: D2D_RECT_F, on: bool, enabled: bool, hit: Hit) -> Result<()> {
        if enabled {
            self.hit(rect, hit, (rect.bottom - rect.top) / 2.0)?;
        }
        self.switch_shape(rect, on)
    }

    /// Windows 11's toggle: a pill, filled with the accent while on, its knob at the end that says which.
    fn switch_shape(&self, rect: D2D_RECT_F, on: bool) -> Result<()> {
        let radius = (rect.bottom - rect.top) / 2.0;
        if on {
            self.fill(rect, radius, self.colors.accent, 1.0)?;
        } else {
            self.outline(rect, radius, self.colors.ink, 0.6, 1.0)?;
        }
        let knob = if on { radius - 5.0 } else { radius - 6.0 };
        let x = if on { rect.right - radius } else { rect.left + radius };
        let ink = if on { self.colors.on_accent } else { self.colors.ink };
        unsafe {
            let brush = self.target.CreateSolidColorBrush(&color(ink, if on { 1.0 } else { 0.75 }), None)?;
            let ellipse = D2D1_ELLIPSE { point: Vector2 { X: x, Y: rect.top + radius }, radiusX: knob, radiusY: knob };
            self.target.FillEllipse(&ellipse, &brush);
        }
        Ok(())
    }

    /// Records a control, with a faint wash while the pointer is over it.
    fn hit(&mut self, rect: D2D_RECT_F, hit: Hit, radius: f32) -> Result<()> {
        if self.hover == Some(self.hits.len()) {
            self.fill(rect, radius, self.colors.ink, 0.05)?;
        }
        self.hits.push((rect, hit));
        Ok(())
    }

    fn fill(&self, rect: D2D_RECT_F, radius: f32, rgb: [f32; 3], alpha: f32) -> Result<()> {
        if alpha <= 0.0 {
            return Ok(());
        }
        unsafe {
            let brush = self.target.CreateSolidColorBrush(&color(rgb, alpha), None)?;
            self.target.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius }, &brush);
        }
        Ok(())
    }

    fn outline(&self, rect: D2D_RECT_F, radius: f32, rgb: [f32; 3], alpha: f32, width: f32) -> Result<()> {
        let inset = D2D_RECT_F { left: rect.left + 0.5, top: rect.top + 0.5, right: rect.right - 0.5, bottom: rect.bottom - 0.5 };
        unsafe {
            let brush = self.target.CreateSolidColorBrush(&color(rgb, alpha), None)?;
            self.target.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: inset, radiusX: radius, radiusY: radius }, &brush, width, None);
        }
        Ok(())
    }

    /// One line of text, centred vertically in `rect`.
    #[allow(clippy::too_many_arguments)]
    fn label(&self, text: &str, rect: D2D_RECT_F, size: f32, bold: bool, rgb: [f32; 3], alpha: f32, align: DWRITE_TEXT_ALIGNMENT) -> Result<()> {
        let weight: DWRITE_FONT_WEIGHT = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        self.text(text, w!("Segoe UI Variable Text"), rect, size, weight, rgb, alpha, align)
    }

    /// A sign, in the symbol font, which draws them in one colour.
    fn glyph(&self, text: &str, rect: D2D_RECT_F, size: f32, rgb: [f32; 3]) -> Result<()> {
        self.text(text, w!("Segoe UI Symbol"), rect, size, DWRITE_FONT_WEIGHT_NORMAL, rgb, 1.0, CENTER)
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        text: &str,
        family: PCWSTR,
        rect: D2D_RECT_F,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        rgb: [f32; 3],
        alpha: f32,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<()> {
        let units: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let format = self.text.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!(""))?;
            format.SetTextAlignment(align)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let brush = self.target.CreateSolidColorBrush(&color(rgb, alpha), None)?;
            self.target.DrawText(&units, &format, &rect, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, Default::default());
        }
        Ok(())
    }
}

const LEADING: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_LEADING;
const CENTER: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_CENTER;
const TRAILING: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_TRAILING;

fn color(rgb: [f32; 3], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: rgb[0], g: rgb[1], b: rgb[2], a: alpha }
}
