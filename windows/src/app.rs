use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::halloween;
use crate::log;
use crate::outside;
use crate::menu::{self, Entry, Icon, Tile};
use crate::settings::{self, Action, COLOR_PRESETS, Source, View};
use crate::prefs::{self, Hotkey, MAX_COLORS, SCHEDULES, SHORTCUTS, SIZES, Settings, WINDS};
use crate::snow::{self, Command, Links};
use crate::text::{fill as fill_in, t};
use crate::tracker::{IGNORED, class_name, process_name};
use crate::update::{self, Release};
use crate::welcome::{self, Handlers};
use snowhush_core::simulation::{Look, Mode};
use snowhush_core::text::{self, tr};
use snowhush_core::weather::{Conditions, LIGHT, season};

const CLASS: PCWSTR = w!("SnowhushController");
const TITLE: PCWSTR = w!("DesktopFlora");
const WM_TRAY: u32 = WM_APP + 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const TIMER_WELCOME: usize = 1;
const TIMER_UPDATES: usize = 2;
/// Every minute: Halloween, the schedule and the seasons follow the clock.
const TIMER_MINUTE: usize = 3;
const TIMER_WEATHER: usize = 4;
const TIMER_CLICK: usize = 5;
const WM_UPDATE: u32 = WM_APP + 2;
const WM_WEATHER: u32 = WM_APP + 3;
const WM_INSTALLED: u32 = WM_APP + 4;
const WEATHER_INTERVAL: u32 = 30 * 60 * 1000;
/// The shell's message for a click on the tray notification.
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;
/// How often DesktopFlora looks for a new version, after the first look shortly after it starts.
const UPDATE_INTERVAL: u32 = 6 * 60 * 60 * 1000;
/// Hotkey ids are the index into `SHORTCUTS` plus this.
const HOTKEY_BASE: i32 = 1;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// A glyph drawn like the Mac's menu bar ones, at the sizes the tray draws it at.
macro_rules! glyph {
    ($name:literal) => {
        [
            (16, include_bytes!(concat!("../assets/tray/", $name, "-16.png")).as_slice()),
            (20, include_bytes!(concat!("../assets/tray/", $name, "-20.png")).as_slice()),
            (24, include_bytes!(concat!("../assets/tray/", $name, "-24.png")).as_slice()),
            (32, include_bytes!(concat!("../assets/tray/", $name, "-32.png")).as_slice()),
            (40, include_bytes!(concat!("../assets/tray/", $name, "-40.png")).as_slice()),
            (48, include_bytes!(concat!("../assets/tray/", $name, "-48.png")).as_slice()),
            (64, include_bytes!(concat!("../assets/tray/", $name, "-64.png")).as_slice()),
        ]
    };
}

type Glyph = [(i32, &'static [u8]); 7];
const SNOWFLAKE_WHITE: Glyph = glyph!("snowflake-white");
const SNOWFLAKE_BLACK: Glyph = glyph!("snowflake-black");
const LEAF_WHITE: Glyph = glyph!("leaf-white");
const LEAF_BLACK: Glyph = glyph!("leaf-black");
const PUMPKIN_WHITE: Glyph = glyph!("pumpkin-white");
const PUMPKIN_BLACK: Glyph = glyph!("pumpkin-black");
const RAIN_WHITE: Glyph = glyph!("rain-white");
const RAIN_BLACK: Glyph = glyph!("rain-black");
const BLOSSOM_WHITE: Glyph = glyph!("blossom-white");
const BLOSSOM_BLACK: Glyph = glyph!("blossom-black");

/// What the tray shows: a snowflake, a jack-o'-lantern while it is Halloween and wanted, a leaf while leaves fall, a
/// rain cloud while it rains, or a cherry blossom while its petals fall.
#[derive(Clone, Copy, PartialEq)]
enum Emblem {
    Snowflake,
    Pumpkin,
    Leaf,
    Rain,
    Blossom,
}

impl Emblem {
    fn of(settings: &Settings, falling: &[Mode]) -> Emblem {
        if falling == [Mode::Snow] && halloween::wanted(settings.halloween) {
            return Emblem::Pumpkin;
        }
        match falling.last().copied().unwrap_or(Mode::Snow) {
            Mode::Snow | Mode::Stars => Emblem::Snowflake,
            Mode::Autumn => Emblem::Leaf,
            Mode::Rain => Emblem::Rain,
            Mode::Blossom => Emblem::Blossom,
        }
    }

    /// Black on a light taskbar, white on a dark one, as the system's own tray icons.
    fn glyph(self, light: bool) -> &'static Glyph {
        match (self, light) {
            (Emblem::Snowflake, false) => &SNOWFLAKE_WHITE,
            (Emblem::Snowflake, true) => &SNOWFLAKE_BLACK,
            (Emblem::Leaf, false) => &LEAF_WHITE,
            (Emblem::Leaf, true) => &LEAF_BLACK,
            (Emblem::Pumpkin, false) => &PUMPKIN_WHITE,
            (Emblem::Pumpkin, true) => &PUMPKIN_BLACK,
            (Emblem::Rain, false) => &RAIN_WHITE,
            (Emblem::Rain, true) => &RAIN_BLACK,
            (Emblem::Blossom, false) => &BLOSSOM_WHITE,
            (Emblem::Blossom, true) => &BLOSSOM_BLACK,
        }
    }
}

/// Whether the taskbar is light: the setting for "your default Windows mode".
fn light_taskbar() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast::<c_void>()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

/// What the snow thread was last told, so only changes are sent.
#[derive(Clone, PartialEq)]
struct Sent {
    intensity: u32,
    modes: Vec<Mode>,
    look: Look,
    shown: bool,
}

struct App {
    settings: Settings,
    commands: Sender<Command>,
    wake: HANDLE,
    snow: Option<JoinHandle<()>>,
    window: HWND,
    icon: HICON,
    emblem: Emblem,
    light: bool,
    locked: bool,
    display_off: bool,
    away: bool,
    /// The update the tray last announced.
    update: Option<Release>,
    /// Programs whose windows get no snow, by lowercase executable name.
    excluded: Vec<String>,
    /// The weather outside and the latitude it was asked for, while following it.
    outside: Option<(Conditions, f64)>,
    /// The country Windows is set to and its latitude, for the seasons without a location.
    home: (String, f64),
    sent: Option<Sent>,
    /// Shortcuts another program already holds.
    taken: [bool; 3],
    double_clicked_at: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// The program last in front other than DesktopFlora and the shell, which the menu offers to keep the snow off.
    static LAST_APP: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The tray icon and its menu, and the system events that pause the snow, on the main thread;
/// the snow itself runs on its own thread so menus and other modal loops never stall it.
pub fn run() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let window = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, TITLE, WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(instance.into()), None)?;
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);

        let settings = Settings::load();
        let (commands, receiver) = mpsc::channel();
        let wake = CreateEventW(None, false, false, None)?;
        let links = Links { wake: wake.0 as isize, controller: window.0 as isize };
        let snow = thread::Builder::new().name("snow".into()).spawn(move || snow::run(receiver, links, settings)).ok();
        let emblem = Emblem::of(&settings, &settings.modes());
        let light = light_taskbar();
        let icon = tray_icon(emblem, light);
        add_tray_icon(window, icon);
        let _ = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
        let _ = RegisterPowerSettingNotification(HANDLE(window.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        if !settings.welcomed {
            SetTimer(Some(window), TIMER_WELCOME, 1200, None);
        }
        SetTimer(Some(window), TIMER_UPDATES, 30_000, None);
        SetTimer(Some(window), TIMER_MINUTE, 60_000, None);
        let _ = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let excluded = prefs::excluded_apps();
        let home = outside::home();
        log!("home: {home:?}");
        let app = App {
            settings,
            commands,
            wake,
            snow,
            window,
            icon,
            emblem,
            light,
            locked: false,
            display_off: false,
            away: false,
            update: None,
            excluded: excluded.clone(),
            outside: None,
            home,
            sent: None,
            taken: [false; 3],
            double_clicked_at: 0,
        };
        send(&app, Command::Excluded(excluded));
        APP.set(Some(app));
        apply();
        register_hotkeys();
        prefs::open_at_login_by_default();
        // Just updated on its own: says so once.
        if let Some(before) = prefs::take_new_version(env!("CARGO_PKG_VERSION")) {
            notify(window, &fill_in(&t("DesktopFlora is now {}"), &[env!("CARGO_PKG_VERSION")]), &fill_in(&t("Updated from {}."), &[&before]));
        }
        if settings.local_weather {
            check_weather(window, false);
            SetTimer(Some(window), TIMER_WEATHER, WEATHER_INTERVAL, None);
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            let now = unsafe { GetTickCount() };
            let double_click = unsafe { GetDoubleClickTime() };
            if event == WM_CONTEXTMENU || event == NIN_KEYSELECT {
                open_menu();
            } else if event == NIN_SELECT {
                // A click may be the first of a double-click: the menu waits until it's clear it isn't.
                let after_double = APP.with_borrow(|app| app.as_ref().is_some_and(|app| now.wrapping_sub(app.double_clicked_at) < double_click));
                if !after_double {
                    unsafe { SetTimer(Some(window), TIMER_CLICK, double_click, None) };
                }
            } else if event == WM_LBUTTONDBLCLK {
                unsafe {
                    let _ = KillTimer(Some(window), TIMER_CLICK);
                }
                APP.with_borrow_mut(|app| {
                    if let Some(app) = app {
                        app.double_clicked_at = now;
                    }
                });
                toggle();
            } else if event == NIN_BALLOONUSERCLICK
                && let Some(release) = APP.with_borrow(|app| app.as_ref().and_then(|app| app.update.clone()))
            {
                offer(window, release);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_CLICK => {
            unsafe {
                let _ = KillTimer(Some(window), TIMER_CLICK);
            }
            open_menu();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_MINUTE => {
            apply();
            LRESULT(0)
        }
        WM_HOTKEY => {
            shortcut(wparam.0 as i32 - HOTKEY_BASE);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WEATHER => {
            check_weather(window, false);
            LRESULT(0)
        }
        WM_WEATHER => {
            let result = unsafe { Box::from_raw(lparam.0 as *mut std::result::Result<(Conditions, f64), String>) };
            weather_arrived(window, *result, wparam.0 != 0);
            LRESULT(0)
        }
        WM_INSTALLED => {
            let result = unsafe { Box::from_raw(lparam.0 as *mut (Release, std::result::Result<(), String>)) };
            let (release, result) = *result;
            // On success the installer closes this copy and opens the new one.
            if let Err(error) = result {
                install_failed(window, &release, &error);
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            update(|app| match wparam.0 as u32 {
                WTS_SESSION_LOCK => app.locked = true,
                WTS_SESSION_UNLOCK => app.locked = false,
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => app.away = true,
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => app.away = false,
                _ => {}
            });
            LRESULT(0)
        }
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_POWERSETTINGCHANGE => {
            let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
            if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                let off = setting.Data[0] == 0;
                update(|app| app.display_off = off);
            }
            LRESULT(1)
        }
        WM_CLOSE => {
            quit(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_UPDATES => {
            unsafe { SetTimer(Some(window), TIMER_UPDATES, UPDATE_INTERVAL, None) };
            check_for_updates(window, false);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            refresh_icon();
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_UPDATE => {
            let result = unsafe { Box::from_raw(lparam.0 as *mut std::result::Result<Option<Release>, String>) };
            updated(window, *result, wparam.0 != 0);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WELCOME => {
            unsafe {
                let _ = KillTimer(Some(window), TIMER_WELCOME);
            }
            welcome::show(Handlers {
                scatter: Box::new(|pieces| with_app(|app| send(app, Command::Scatter(pieces)))),
                finished: Box::new(|| {
                    APP.with_borrow_mut(|app| {
                        let Some(app) = app else { return };
                        app.settings.welcomed = true;
                        app.settings.save();
                    })
                }),
                skipped: Box::new(|| with_app(|app| send(app, Command::ReleaseSnowfall))),
            });
            LRESULT(0)
        }
        _ if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with_app(|app| {
                add_tray_icon(window, app.icon);
                send(app, Command::ShellRestarted);
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Changes what pauses the snow and tells the snow thread whether it may run.
fn update(change: impl FnOnce(&mut App)) {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        change(app);
        let active = !app.locked && !app.display_off && !app.away;
        send(app, Command::Active(active));
    });
}

fn send(app: &App, command: Command) {
    let _ = app.commands.send(command);
    unsafe {
        let _ = SetEvent(app.wake);
    }
}

fn with_app(action: impl FnOnce(&App)) {
    APP.with_borrow(|app| {
        if let Some(app) = app {
            action(app);
        }
    });
}

/// What falls now and how much, in percent, and the breeze's share from outside: from the menu, the sky outside or the
/// season where the person is.
fn effective(app: &App, month: u32) -> (u32, Vec<Mode>, f32) {
    let settings = &app.settings;
    let latitude = app.outside.filter(|_| settings.local_weather).map_or(app.home.1, |(_, latitude)| latitude);
    let seasonal = season(month, latitude);
    match app.outside {
        Some((outside, _)) if settings.local_weather => {
            // Under a dry sky the season's weather falls lightly instead: leaves in autumn, stars on summer nights.
            let (intensity, mode) = outside.falling.unwrap_or((LIGHT, seasonal));
            (intensity, vec![mode], outside.wind)
        }
        _ if settings.local_weather || settings.seasonal => (settings.intensity, vec![seasonal], 1.0),
        _ => (settings.intensity, settings.modes(), 1.0),
    }
}

/// Works out what should fall now and whether it shows, and tells the snow thread whatever changed.
fn apply() {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        let now = unsafe { GetLocalTime() };
        let (intensity, modes, outside_wind) = effective(app, now.wMonth as u32);
        let mut look = app.settings.look();
        look.wind *= outside_wind;
        let shown = !app.settings.paused && app.settings.scheduled(now.wHour as u32);
        let next = Sent { intensity, modes: modes.clone(), look: look.clone(), shown };
        let sent = app.sent.clone();
        if sent.as_ref().map(|s| s.intensity) != Some(intensity) {
            send(app, Command::Intensity(intensity as f32 / 100.0));
        }
        if sent.as_ref().map(|s| &s.modes) != Some(&modes) {
            let mixed = modes.len() > 1;
            let mix = if mixed { [modes[0], modes[1]] } else { app.settings.mix };
            let mode = *modes.last().unwrap_or(&Mode::Snow);
            send(app, Command::SwitchMode { mode, mixed, mix, calm: !prefs::animations_enabled() });
        }
        if sent.as_ref().map(|s| &s.look) != Some(&look) {
            send(app, Command::Look(look));
        }
        if sent.as_ref().map(|s| s.shown) != Some(shown) {
            send(app, Command::Shown(shown));
        }
        // Halloween comes and goes with the date: the snow thread checks it itself, the tray here.
        send(app, Command::Halloween(app.settings.halloween));
        app.sent = Some(next);
    });
    refresh_icon();
    settings::refresh();
}

/// On or off: for the menu, the double-click and the shortcut.
fn toggle() {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        app.settings.paused = !app.settings.paused;
        app.settings.save();
    });
    apply();
}

fn shortcut(index: i32) {
    match index {
        0 => toggle(),
        1 => with_app(|app| send(app, Command::ClearSnow(!prefs::animations_enabled()))),
        2 => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                let now = unsafe { GetLocalTime() };
                let current = effective(app, now.wMonth as u32).1.last().copied().unwrap_or(Mode::Snow);
                let next = Mode::ALL.iter().position(|&m| m == current).map_or(0, |i| (i + 1) % Mode::ALL.len());
                app.settings.mode = Mode::ALL[next];
                app.settings.mixed = false;
                app.settings.seasonal = false;
                app.settings.local_weather = false;
                app.settings.paused = false;
                app.outside = None;
                app.settings.save();
            });
            apply();
        }
        _ => {}
    }
}

/// Registers the shortcuts set in the menu, noting which another program already holds.
fn register_hotkeys() {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        for (i, hotkey) in app.settings.hotkeys.iter().enumerate() {
            let id = HOTKEY_BASE + i as i32;
            unsafe {
                let _ = UnregisterHotKey(Some(app.window), id);
            }
            app.taken[i] = match hotkey {
                Some(hotkey) => {
                    let modifiers = HOT_KEY_MODIFIERS(hotkey.modifiers | MOD_NOREPEAT.0);
                    let taken = unsafe { RegisterHotKey(Some(app.window), id, modifiers, hotkey.key) }.is_err();
                    if taken {
                        log!("shortcut {} is taken by another program", describe(*hotkey));
                    }
                    taken
                }
                None => false,
            };
        }
    });
}

/// Shows the tray menu at the pointer: Snowhush's drawn glass menu, or the system's when that can't be had.
fn open_menu() {
    let Some(window) = APP.with_borrow(|app| app.as_ref().map(|app| app.window)) else { return };
    let mut cursor = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    let (x, y) = (cursor.x, cursor.y);
    let Some(current) = entries(true) else { return };
    let mut toggle = |id: u32| {
        choose(window, id);
        entries(true).unwrap_or_default()
    };
    let native = || native_menu(window, x, y, &entries(false).unwrap_or_default());
    let chosen = if menu::available() {
        menu::track(x, y, current, light_taskbar(), &mut toggle).unwrap_or_else(|error| {
            log!("menu failed, using the system's: {error}");
            native()
        })
    } else {
        native()
    };
    choose(window, chosen);
}

const ID_SNOWFALL: u32 = 100;
const INTENSITY_STEPS: u32 = 21;
const INTENSITY_STOPS: [u32; 5] = [0, 25, 50, 75, 100];
const ID_OPACITY: u32 = 400;
const OPACITY_STEPS: u32 = 19;
const OPACITY_STOPS: [u32; 5] = [100, 80, 60, 40, 20];
const ID_ACCUMULATE: u32 = 200;
const ID_MELT: u32 = 201;
const ID_SCREENSHOTS: u32 = 202;
const ID_LOGIN: u32 = 203;
const ID_WINDOWS: u32 = 204;
const ID_BEHIND: u32 = 205;
const ID_UPDATES: u32 = 206;
const ID_CLEAR: u32 = 207;
const ID_HALLOWEEN: u32 = 208;
const ID_TASKBAR: u32 = 209;
const ID_FULL_SCREEN: u32 = 212;
const ID_DEEP: u32 = 216;
const ID_MIXED: u32 = 219;
const ID_GROUND: u32 = 220;
const ID_OPACITY_FOLD: u32 = 221;
const ID_PAUSE: u32 = 222;
const ID_TYPING: u32 = 223;
const ID_SEASONAL: u32 = 224;
const ID_OUTSIDE: u32 = 225;
const ID_NATURAL: u32 = 226;
const ID_EXCLUDE: u32 = 227;
const ID_SETTINGS: u32 = 228;
const ID_QUIT: u32 = 299;
/// One per kind in `Mode::ALL`.
const ID_MODE: u32 = 230;
const ID_SIZE: u32 = 240;
const ID_WIND: u32 = 250;
const ID_SCHEDULE: u32 = 260;
const ID_RATE: u32 = 270;
const ID_COLOR: u32 = 280;
/// Each program kept snow-free, from here up: choosing one lets the snow back on it.
const ID_EXCLUDED: u32 = 500;
/// The shortcuts' choices: `ID_SHORTCUT` + 10 × the shortcut + its choice in `SHORTCUT_CHOICES`.
const ID_SHORTCUT: u32 = 600;
/// Colours to pick for snow and stars, up to `MAX_COLORS` at once.
/// The keys each shortcut can be put on: Ctrl+Alt, Ctrl+Shift or Win+Alt with its letter, or none.
const SHORTCUT_CHOICES: [u32; 3] = [0x0002 | 0x0001, 0x0002 | 0x0004, 0x0008 | 0x0001];

thread_local! {
    /// Whether Accumulate and Opacity show what they fold, for as long as the app runs.
    static ACCUMULATE_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static OPACITY_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What the tray menu lists, with what is on or off: sliders in the drawn menu, submenus in the system's.
fn entries(sliders: bool) -> Option<Vec<Entry>> {
    APP.with_borrow(|app| {
        let app = app.as_ref()?;
        let settings = app.settings;
        let now = unsafe { GetLocalTime() };
        let (intensity, falling, _) = effective(app, now.wMonth as u32);
        let chosen = !settings.local_weather && !settings.seasonal;
        let picked = settings.modes();

        // What falls: picked here, mixed, or following the season or the sky outside.
        let kind = |mode: Mode| {
            let id = ID_MODE + Mode::ALL.iter().position(|&m| m == mode).unwrap_or(0) as u32;
            if settings.mixed && chosen {
                Entry::toggle(id, mode_name(mode), picked.contains(&mode), true)
            } else {
                Entry::choice(id, mode_name(mode), chosen && settings.mode == mode)
            }
        };
        let mut modes = vec![Entry::toggle(ID_MIXED, tr("mixed"), settings.mixed && chosen, true), Entry::Separator];
        modes.extend(Mode::ALL.iter().map(|&mode| kind(mode)));
        modes.extend([
            Entry::Separator,
            Entry::choice(ID_SEASONAL, &t("Season"), settings.seasonal && !settings.local_weather),
            Entry::choice(ID_OUTSIDE, &t("Outside"), settings.local_weather),
        ]);
        if let Some(status) = status(app, &falling) {
            modes.push(Entry::header(&status));
        }
        modes.extend([Entry::Separator, Entry::item(ID_HALLOWEEN, tr("halloween"), halloween::wanted(settings.halloween), falling == [Mode::Snow])]);

        // Rain and stars don't lie anywhere: nothing to keep, melt or clear.
        let heaps: Vec<Mode> = falling.iter().copied().filter(|&mode| mode != Mode::Rain && mode != Mode::Stars).collect();
        let amount = if intensity == 0 { tr("off").to_string() } else { format!("{intensity}%") };
        let mut entries = vec![Entry::header("DesktopFlora"), Entry::toggle(ID_PAUSE, &t("Snow on"), !settings.paused, true), Entry::Separator];
        if sliders {
            entries.extend([
                Entry::header(tr("intensity")),
                Entry::slider(ID_SNOWFALL, intensity.clamp(0, 100) / 5, INTENSITY_STEPS, &amount),
                // Opacity is set once and left, so it folds away like Accumulate, its value on the row.
                Entry::Disclosure {
                    id: ID_OPACITY_FOLD,
                    label: tr("opacity").to_string(),
                    summary: format!("{}%", settings.opacity),
                    icons: Vec::new(),
                    open: OPACITY_OPEN.get(),
                    keeps_summary: true,
                },
                Entry::folded_slider(ID_OPACITY, (settings.opacity.clamp(10, 100) - 10) / 5, OPACITY_STEPS, OPACITY_OPEN.get()),
                Entry::Separator,
            ]);
        } else {
            let intensities = INTENSITY_STOPS.iter().map(|&stop| Entry::choice(ID_SNOWFALL + stop / 5, &percent(stop), stop == intensity)).collect();
            let opacities =
                OPACITY_STOPS.iter().map(|&stop| Entry::choice(ID_OPACITY + (stop - 10) / 5, &format!("{stop}%"), stop == settings.opacity)).collect();
            entries.extend([
                Entry::Submenu { label: tr("intensity").to_string(), entries: intensities },
                Entry::Submenu { label: tr("opacity").to_string(), entries: opacities },
            ]);
        }
        entries.extend(accumulate(&settings, !heaps.is_empty(), sliders));
        entries.push(Entry::Submenu { label: tr("mode").to_string(), entries: modes });
        if !heaps.is_empty() {
            entries.push(Entry::item(ID_CLEAR, text::kinds("clear", &heaps), false, true));
        }
        entries.extend([
            Entry::Separator,
            Entry::item(ID_SETTINGS, &t("Settings…"), false, true),
            Entry::item(ID_QUIT, tr("quit"), false, true),
        ]);
        Some(entries)
    })
}


/// Where snow settles: a row of icons Accumulate folds out in the drawn menu, a submenu of checkmarks in the system's.
fn accumulate(settings: &Settings, heaps: bool, drawn: bool) -> Vec<Entry> {
    let mut surfaces = vec![(ID_WINDOWS, Icon::Windows, tr("surface.windows"), settings.snow_on_windows)];
    if heaps {
        surfaces.push((ID_GROUND, Icon::Ground, tr("surface.ground"), settings.ground));
    }
    if !drawn {
        let entries = surfaces.iter().map(|&(id, _, label, on)| Entry::item(id, label, on, true)).collect();
        return vec![Entry::Submenu { label: tr("accumulate").to_string(), entries }];
    }
    let on: Vec<Icon> = surfaces.iter().filter(|surface| surface.3).map(|surface| surface.1).collect();
    // Some on, the row shows their icons rather than their names, which would widen the menu.
    let (summary, icons) = if on.len() == surfaces.len() {
        (tr("all").to_string(), Vec::new())
    } else if on.is_empty() {
        (tr("off").to_string(), Vec::new())
    } else {
        (String::new(), on)
    };
    let open = ACCUMULATE_OPEN.get();
    let tiles = surfaces.into_iter().map(|(id, icon, label, on)| Tile { id, icon, label: label.to_string(), on }).collect();
    vec![
        Entry::Disclosure { id: ID_ACCUMULATE, label: tr("accumulate").to_string(), summary, icons, open, keeps_summary: false },
        Entry::Icons { tiles, shown: open },
    ]
}



/// A line under Mode on where what falls comes from, while it follows the season or the sky.
fn status(app: &App, falling: &[Mode]) -> Option<String> {
    let kind = falling.last().copied().unwrap_or(Mode::Snow);
    let season_name = match kind {
        Mode::Snow => t("Winter"),
        Mode::Blossom => t("Spring"),
        Mode::Stars => t("Summer"),
        Mode::Autumn => t("Autumn"),
        Mode::Rain => String::new(),
    };
    if app.settings.local_weather {
        Some(match app.outside {
            Some((Conditions { falling: Some(_), .. }, _)) => fill_in(&t("Outside right now: {}"), &[mode_name(kind)]),
            Some(_) => fill_in(&t("Dry outside: {} for the {}"), &[mode_name(kind), &season_name]),
            None => t("Asking for the weather outside…"),
        })
    } else if app.settings.seasonal {
        let place = if app.home.0.is_empty() { t("your region") } else { app.home.0.clone() };
        Some(format!("{season_name} · {place}"))
    } else {
        None
    }
}

fn mode_name(mode: Mode) -> &'static str {
    tr(match mode {
        Mode::Snow => "snow",
        Mode::Autumn => "autumn",
        Mode::Rain => "rain",
        Mode::Blossom => "blossom",
        Mode::Stars => "stars",
    })
}

/// An intensity as its caption: a percentage, or Off.
fn percent(intensity: u32) -> String {
    if intensity == 0 { tr("off").to_string() } else { format!("{intensity}%") }
}

/// A shortcut as people read it, as "Ctrl+Alt+S".
fn describe(hotkey: Hotkey) -> String {
    let mut text = String::new();
    for (flag, name) in [(0x0002, "Ctrl"), (0x0001, "Alt"), (0x0004, "Shift"), (0x0008, "Win")] {
        if hotkey.modifiers & flag != 0 {
            text.push_str(&t(name));
            text.push('+');
        }
    }
    text.push(char::from_u32(hotkey.key).unwrap_or('?'));
    text
}

/// The system's own menu, for when the drawn one can't be shown.
fn native_menu(window: HWND, x: i32, y: i32, entries: &[Entry]) -> u32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        append(menu, entries);
        // A tray menu only closes on an outside click while its owner is in the foreground.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0, x, y, window, None).0 as u32;
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen
    }
}

unsafe fn append(menu: HMENU, entries: &[Entry]) {
    let text = |label: &str| -> Vec<u16> { label.encode_utf16().chain([0]).collect() };
    for entry in entries {
        unsafe {
            match entry {
                Entry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                Entry::Header(label) => {
                    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(text(label).as_ptr()));
                }
                Entry::Item { id, label, checked, enabled, radio, .. } => {
                    let flags = MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED } | if *enabled { MF_ENABLED } else { MF_GRAYED };
                    let _ = AppendMenuW(menu, flags, *id as usize, PCWSTR(text(label).as_ptr()));
                    if *radio && *checked {
                        let _ = CheckMenuRadioItem(menu, *id, *id, *id, MF_BYCOMMAND.0);
                    }
                }
                Entry::Submenu { label, entries } => {
                    let Ok(submenu) = CreatePopupMenu() else { continue };
                    append(submenu, entries);
                    let _ = AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR(text(label).as_ptr()));
                }
                Entry::Slider { .. } | Entry::Disclosure { .. } | Entry::Icons { .. } => {}
            }
        }
    }
}

/// What choosing an entry does.
fn choose(window: HWND, chosen: u32) {
    match chosen {
        0 => return,
        ID_QUIT => return quit(window),
        ID_UPDATES => return check_for_updates(window, true),
        ID_PAUSE => return toggle(),
        ID_SETTINGS => return open_settings(),
        ID_CLEAR => return with_app(|app| send(app, Command::ClearSnow(!prefs::animations_enabled()))),
        ID_ACCUMULATE => return ACCUMULATE_OPEN.set(!ACCUMULATE_OPEN.get()),
        ID_OPACITY_FOLD => return OPACITY_OPEN.set(!OPACITY_OPEN.get()),
        ID_LOGIN => {
            let on = !prefs::opens_at_login();
            let status = prefs::set_opens_at_login(on);
            log!("open at login {on}: {status:?}");
            return;
        }
        ID_EXCLUDE => {
            if let Some(last) = LAST_APP.with_borrow(Clone::clone) {
                set_excluded(|apps| {
                    if !apps.contains(&last) {
                        apps.push(last);
                    }
                });
            }
            return;
        }
        id if (ID_EXCLUDED..ID_SHORTCUT).contains(&id) => {
            let i = (id - ID_EXCLUDED) as usize;
            return set_excluded(|apps| {
                if i < apps.len() {
                    apps.remove(i);
                }
            });
        }
        id if (ID_SHORTCUT..ID_SHORTCUT + 10 * SHORTCUTS.len() as u32).contains(&id) => {
            let (i, c) = (((id - ID_SHORTCUT) / 10) as usize, ((id - ID_SHORTCUT) % 10) as usize);
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                app.settings.hotkeys[i] = SHORTCUT_CHOICES.get(c).map(|&modifiers| Hotkey { modifiers, key: SHORTCUTS[i].2.key });
                app.settings.save();
            });
            return register_hotkeys();
        }
        ID_OUTSIDE => {
            if !APP.with_borrow(|app| app.as_ref().is_some_and(|app| app.settings.local_weather)) {
                set_local_weather(window, true);
                check_weather(window, true);
            }
            return;
        }
        id if (ID_RATE..ID_RATE + 3).contains(&id) => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                app.settings.eco = id == ID_RATE;
                app.settings.max_performance = id == ID_RATE + 2;
                app.settings.save();
                send(app, Command::Performance(app.settings.max_performance));
                send(app, Command::RateCap(app.settings.rate_cap()));
            });
            return;
        }
        _ => {}
    }
    let command = APP.with_borrow_mut(|app| {
        let app = app.as_mut()?;
        let settings = &mut app.settings;
        let command = match chosen {
            id if (ID_SNOWFALL..ID_SNOWFALL + INTENSITY_STEPS).contains(&id) => {
                settings.intensity = 5 * (id - ID_SNOWFALL);
                // Choosing how much by hand stops following the weather outside.
                settings.local_weather = false;
                app.outside = None;
                None
            }
            id if (ID_OPACITY..ID_OPACITY + OPACITY_STEPS).contains(&id) => {
                settings.opacity = 10 + 5 * (id - ID_OPACITY);
                Some(Command::FallingOpacity(settings.opacity as f32 / 100.0))
            }
            id if (ID_MODE..ID_MODE + Mode::ALL.len() as u32).contains(&id) => {
                let mode = Mode::ALL[(id - ID_MODE) as usize];
                // Picked from following the season or the sky, a kind falls alone.
                if settings.seasonal || settings.local_weather {
                    settings.mode = mode;
                } else {
                    settings.pick(mode);
                }
                settings.seasonal = false;
                settings.local_weather = false;
                settings.paused = false;
                app.outside = None;
                None
            }
            ID_MIXED => {
                settings.seasonal = false;
                settings.local_weather = false;
                app.outside = None;
                settings.toggle_mixed();
                None
            }
            ID_SEASONAL => {
                settings.seasonal = true;
                settings.local_weather = false;
                settings.paused = false;
                app.outside = None;
                None
            }
            ID_HALLOWEEN => {
                settings.halloween = settings.halloween.toggled(halloween::today());
                None
            }
            id if (ID_SIZE..ID_SIZE + SIZES.len() as u32).contains(&id) => {
                settings.size = (id - ID_SIZE) as usize;
                None
            }
            id if (ID_WIND..ID_WIND + WINDS.len() as u32).contains(&id) => {
                settings.wind = (id - ID_WIND) as usize;
                None
            }
            id if (ID_SCHEDULE..ID_SCHEDULE + SCHEDULES.len() as u32).contains(&id) => {
                settings.schedule = (id - ID_SCHEDULE) as usize;
                None
            }
            ID_NATURAL => {
                settings.custom_colors = false;
                None
            }
            id if (ID_COLOR..ID_COLOR + COLOR_PRESETS.len() as u32).contains(&id) => {
                let rgb = COLOR_PRESETS[(id - ID_COLOR) as usize].1;
                let count = settings.palette_len.min(MAX_COLORS);
                if !settings.custom_colors {
                    // From natural colours, the first colour picked starts a fresh palette.
                    settings.custom_colors = true;
                    settings.palette_len = 0;
                }
                if let Some(i) = settings.palette[..settings.palette_len.min(count)].iter().position(|c| *c == rgb) {
                    settings.palette.copy_within(i + 1..settings.palette_len, i);
                    settings.palette_len -= 1;
                } else if settings.palette_len < MAX_COLORS {
                    settings.palette[settings.palette_len] = rgb;
                    settings.palette_len += 1;
                }
                None
            }
            ID_GROUND => {
                settings.ground = !settings.ground;
                Some(Command::Ground(settings.ground))
            }
            ID_WINDOWS => {
                settings.snow_on_windows = !settings.snow_on_windows;
                Some(Command::SnowOnWindows(settings.snow_on_windows))
            }
            ID_MELT => {
                settings.melt = !settings.melt;
                Some(Command::Melt(settings.melt))
            }
            ID_DEEP => {
                settings.deep_interaction = !settings.deep_interaction;
                Some(Command::DeepInteraction(settings.deep_interaction))
            }
            ID_BEHIND => {
                settings.behind_windows = !settings.behind_windows;
                Some(Command::BehindWindows(settings.behind_windows))
            }
            ID_FULL_SCREEN => {
                settings.over_full_screen = !settings.over_full_screen;
                Some(Command::OverFullScreen(settings.over_full_screen))
            }
            ID_TASKBAR => {
                settings.behind_taskbar = !settings.behind_taskbar;
                Some(Command::BehindTaskbar(settings.behind_taskbar))
            }
            ID_SCREENSHOTS => {
                settings.show_in_screenshots = !settings.show_in_screenshots;
                Some(Command::ShowInScreenshots(settings.show_in_screenshots))
            }
            ID_TYPING => {
                settings.fade_while_typing = !settings.fade_while_typing;
                Some(Command::FadeWhileTyping(settings.fade_while_typing))
            }
            _ => return Some(None),
        };
        app.settings.save();
        Some(command)
    });
    if let Some(Some(command)) = command {
        with_app(|app| send(app, command));
    }
    // What falls, how much, how it looks and whether it shows go through here, which sends only what changed.
    apply();
    if !APP.with_borrow(|app| app.as_ref().is_some_and(|app| app.settings.local_weather)) {
        unsafe {
            let _ = KillTimer(Some(window), TIMER_WEATHER);
        }
    }
}

fn open_settings() {
    settings::show(settings::Handlers { view: Box::new(view), act: Box::new(act) });
}

/// What the settings window shows.
fn view() -> Option<View> {
    let login = prefs::opens_at_login();
    let last_app = LAST_APP.with_borrow(Clone::clone);
    APP.with_borrow(|app| {
        let app = app.as_ref()?;
        let settings = app.settings;
        let now = unsafe { GetLocalTime() };
        let (intensity, falling, _) = effective(app, now.wMonth as u32);
        let kind = falling.last().copied().unwrap_or(Mode::Snow);
        let season_name = |mode: Mode| match mode {
            Mode::Snow => t("Winter"),
            Mode::Blossom => t("Spring"),
            Mode::Stars => t("Summer"),
            Mode::Autumn => t("Autumn"),
            Mode::Rain => String::new(),
        };
        let name = |mode: Mode| {
            tr(match mode {
                Mode::Snow => "snow",
                Mode::Autumn => "autumn",
                Mode::Rain => "rain",
                Mode::Blossom => "blossom",
                Mode::Stars => "stars",
            })
            .to_string()
        };
        let status = if settings.local_weather {
            let first = match app.outside {
                Some((Conditions { falling: Some(_), .. }, _)) => fill_in(&t("Outside right now: {}"), &[&name(kind)]),
                Some(_) => fill_in(&t("Dry outside: {} for the {}"), &[&name(kind), &season_name(kind)]),
                None => t("Asking for the weather outside…"),
            };
            (first, t("Place: your location from Windows, rounded to about 1 km"))
        } else if settings.seasonal {
            let place = if app.home.0.is_empty() { t("your region") } else { app.home.0.clone() };
            (format!("{} · {}", name(kind), season_name(kind)), fill_in(&t("Season in {}, from the region set in Windows"), &[&place]))
        } else if !settings.scheduled(now.wHour as u32) {
            (t("Outside the schedule right now"), String::new())
        } else {
            (String::new(), String::new())
        };
        Some(View {
            on: !settings.paused,
            settings,
            halloween_possible: falling == [Mode::Snow],
            halloween_on: halloween::wanted(settings.halloween),
            falling,
            intensity,
            status,
            excluded: app.excluded.clone(),
            last_app: last_app.filter(|last| !app.excluded.contains(last)),
            login,
            taken: app.taken,
        })
    })
}

/// What a click in the settings window does.
fn act(action: Action) {
    let window = APP.with_borrow(|app| app.as_ref().map(|app| app.window));
    let Some(window) = window else { return };
    match action {
        Action::Toggle => return toggle(),
        Action::Quit => return quit(window),
        Action::Updates => return check_for_updates(window, true),
        Action::Clear => return with_app(|app| send(app, Command::ClearSnow(!prefs::animations_enabled()))),
        Action::OpenAtLogin => {
            let on = !prefs::opens_at_login();
            let status = prefs::set_opens_at_login(on);
            log!("open at login {on}: {status:?}");
            return;
        }
        Action::Exclude => {
            if let Some(last) = LAST_APP.with_borrow(Clone::clone) {
                set_excluded(|apps| {
                    if !apps.contains(&last) {
                        apps.push(last);
                    }
                });
            }
            return;
        }
        Action::Include(i) => {
            return set_excluded(|apps| {
                if i < apps.len() {
                    apps.remove(i);
                }
            });
        }
        Action::Source(Source::Outside) => {
            if !APP.with_borrow(|app| app.as_ref().is_some_and(|app| app.settings.local_weather)) {
                set_local_weather(window, true);
                check_weather(window, true);
            }
            return;
        }
        Action::Rate(i) => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                app.settings.eco = i == 0;
                app.settings.max_performance = i == 2;
                app.settings.save();
                send(app, Command::Performance(app.settings.max_performance));
                send(app, Command::RateCap(app.settings.rate_cap()));
            });
            return;
        }
        Action::SuspendShortcuts(on) => {
            if on {
                with_app(|app| {
                    for i in 0..SHORTCUTS.len() {
                        unsafe {
                            let _ = UnregisterHotKey(Some(app.window), HOTKEY_BASE + i as i32);
                        }
                    }
                });
            } else {
                register_hotkeys();
            }
            return;
        }
        Action::SetHotkey(i, hotkey) => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                if i < SHORTCUTS.len() {
                    app.settings.hotkeys[i] = hotkey;
                    app.settings.save();
                }
            });
            register_hotkeys();
            return;
        }
        _ => {}
    }
    let command = APP.with_borrow_mut(|app| {
        let app = app.as_mut()?;
        let settings = &mut app.settings;
        let command = match action {
            Action::Intensity(value) => {
                settings.intensity = value.min(100);
                // Choosing how much by hand stops following the weather outside.
                settings.local_weather = false;
                app.outside = None;
                None
            }
            Action::Opacity(value) => {
                settings.opacity = value.clamp(10, 100);
                Some(Command::FallingOpacity(settings.opacity as f32 / 100.0))
            }
            Action::Pick(mode) => {
                settings.pick(mode);
                settings.seasonal = false;
                settings.local_weather = false;
                settings.paused = false;
                app.outside = None;
                None
            }
            Action::Mixed => {
                settings.toggle_mixed();
                None
            }
            Action::Halloween => {
                settings.halloween = settings.halloween.toggled(halloween::today());
                None
            }
            Action::Source(source) => {
                settings.seasonal = source == Source::Season;
                settings.local_weather = false;
                settings.paused = false;
                app.outside = None;
                None
            }
            Action::Size(i) => {
                settings.size = i.min(SIZES.len() - 1);
                None
            }
            Action::Wind(i) => {
                settings.wind = i.min(WINDS.len() - 1);
                None
            }
            Action::CustomColors(on) => {
                settings.custom_colors = on;
                None
            }
            Action::Color(i) => {
                let rgb = COLOR_PRESETS[i.min(COLOR_PRESETS.len() - 1)].1;
                if let Some(at) = settings.palette[..settings.palette_len.min(MAX_COLORS)].iter().position(|c| *c == rgb) {
                    settings.palette.copy_within(at + 1..settings.palette_len, at);
                    settings.palette_len -= 1;
                } else if settings.palette_len < MAX_COLORS {
                    settings.palette[settings.palette_len] = rgb;
                    settings.palette_len += 1;
                }
                None
            }
            Action::Schedule(i) => {
                settings.schedule = i.min(SCHEDULES.len() - 1);
                None
            }
            Action::Ground => {
                settings.ground = !settings.ground;
                Some(Command::Ground(settings.ground))
            }
            Action::SnowOnWindows => {
                settings.snow_on_windows = !settings.snow_on_windows;
                Some(Command::SnowOnWindows(settings.snow_on_windows))
            }
            Action::Melt => {
                settings.melt = !settings.melt;
                Some(Command::Melt(settings.melt))
            }
            Action::DeepInteraction => {
                settings.deep_interaction = !settings.deep_interaction;
                Some(Command::DeepInteraction(settings.deep_interaction))
            }
            Action::BehindWindows => {
                settings.behind_windows = !settings.behind_windows;
                Some(Command::BehindWindows(settings.behind_windows))
            }
            Action::OverFullScreen => {
                settings.over_full_screen = !settings.over_full_screen;
                Some(Command::OverFullScreen(settings.over_full_screen))
            }
            Action::BehindTaskbar => {
                settings.behind_taskbar = !settings.behind_taskbar;
                Some(Command::BehindTaskbar(settings.behind_taskbar))
            }
            Action::Screenshots => {
                settings.show_in_screenshots = !settings.show_in_screenshots;
                Some(Command::ShowInScreenshots(settings.show_in_screenshots))
            }
            Action::FadeWhileTyping => {
                settings.fade_while_typing = !settings.fade_while_typing;
                Some(Command::FadeWhileTyping(settings.fade_while_typing))
            }
            _ => None,
        };
        app.settings.save();
        Some(command)
    });
    if let Some(Some(command)) = command {
        with_app(|app| send(app, command));
    }
    // What falls, how much, how it looks and whether it shows go through here, which sends only what changed.
    apply();
    if !APP.with_borrow(|app| app.as_ref().is_some_and(|app| app.settings.local_weather)) {
        unsafe {
            let _ = KillTimer(Some(window), TIMER_WEATHER);
        }
    }
}

/// Remembers the program that came to the front, unless it is DesktopFlora or part of the shell.
unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, window: HWND, _: i32, _: i32, _: u32, _: u32) {
    let class = class_name(window);
    if IGNORED.contains(&class.as_str()) || class == "DesktopFloraMenu" {
        return;
    }
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    if process == 0 || process == std::process::id() {
        return;
    }
    let name = process_name(process).to_lowercase();
    // Without the right to read its name, a process shows as its number, which can't be kept off.
    if name.ends_with(".exe") && name != "explorer.exe" {
        LAST_APP.set(Some(name));
    }
}

fn set_excluded(change: impl FnOnce(&mut Vec<String>)) {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        change(&mut app.excluded);
        prefs::set_excluded_apps(&app.excluded);
        send(app, Command::Excluded(app.excluded.clone()));
    });
}

fn set_local_weather(window: HWND, on: bool) {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        app.settings.local_weather = on;
        if on {
            app.settings.paused = false;
        }
        app.settings.save();
        if !on {
            app.outside = None;
        }
    });
    apply();
    unsafe {
        if on {
            SetTimer(Some(window), TIMER_WEATHER, WEATHER_INTERVAL, None);
        } else {
            let _ = KillTimer(Some(window), TIMER_WEATHER);
        }
    }
}

/// Asks for the weather outside on another thread and reports back to the window with `WM_WEATHER`.
fn check_weather(window: HWND, asked: bool) {
    let target = window.0 as isize;
    thread::spawn(move || {
        // WinRT needs the thread in an apartment before the location can be asked for.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let result = Box::new(outside::now());
        unsafe {
            let _ = PostMessageW(Some(HWND(target as _)), WM_WEATHER, WPARAM(asked as usize), LPARAM(Box::into_raw(result) as isize));
        }
    });
}

fn weather_arrived(window: HWND, result: std::result::Result<(Conditions, f64), String>, asked: bool) {
    match result {
        Ok(outside) => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                if app.settings.local_weather {
                    log!("outside: {outside:?}");
                    app.outside = Some(outside);
                }
            });
            apply();
        }
        Err(error) => {
            log!("weather failed: {error}");
            // A check that fails later keeps the last weather; one that fails as it's turned on turns it off again.
            if asked {
                set_local_weather(window, false);
                message(window, &fill_in(&t("Couldn't get the local weather.\n\n{}"), &[&error]), MB_OK | MB_ICONWARNING);
            }
        }
    }
}

/// Checks on another thread and reports back to the window with `WM_UPDATE`.
fn check_for_updates(window: HWND, asked: bool) {
    let target = window.0 as isize;
    thread::spawn(move || {
        let result = Box::new(update::check());
        unsafe {
            let _ = PostMessageW(Some(HWND(target as _)), WM_UPDATE, WPARAM(asked as usize), LPARAM(Box::into_raw(result) as isize));
        }
    });
}

/// A check someone asked for always answers; the daily one only speaks up, once, about a new version.
fn updated(window: HWND, result: std::result::Result<Option<Release>, String>, asked: bool) {
    let current = env!("CARGO_PKG_VERSION");
    match result {
        Ok(Some(release)) if asked => offer(window, release),
        Ok(Some(release)) => {
            let fresh = APP.with_borrow_mut(|app| {
                let app = app.as_mut()?;
                (app.update.as_ref().map(|known| &known.url) != Some(&release.url)).then(|| app.update = Some(release.clone()))
            });
            if fresh.is_none() {
                return;
            }
            // With a checksum to check it against, the update installs itself and DesktopFlora starts again; without,
            // the tray only says it is there.
            if release.digest.is_some() {
                log!("updating to {} on its own", release.version);
                install_in_background(window, release);
            } else {
                notify(window, &text::fill("update.available", &[("version", &release.version)]), &t("Click to install it."));
            }
        }
        Ok(None) if asked => {
            message(window, &text::fill("update.latest", &[("current", current)]), MB_OK | MB_ICONINFORMATION);
        }
        Ok(None) => {}
        Err(error) => {
            log!("update check failed: {error}");
            if asked {
                message(window, &format!("{}\n\n{error}", tr("update.failed")), MB_OK | MB_ICONWARNING);
            }
        }
    }
}

/// Offers a new version: installed in place when its release publishes a checksum to check the download against,
/// otherwise from its page in the browser.
fn offer(window: HWND, release: Release) {
    let current = env!("CARGO_PKG_VERSION");
    if release.digest.is_none() {
        let text = text::fill("update.ask", &[("version", &release.version), ("current", current)]);
        if message(window, &text, MB_YESNO | MB_ICONINFORMATION) == IDYES {
            update::open(&release.page);
        }
        return;
    }
    let text = fill_in(
        &t("DesktopFlora {} is available. You have {}.\n\nInstall it now? DesktopFlora downloads it, checks it against the release's checksum and restarts."),
        &[&release.version, current],
    );
    if message(window, &text, MB_YESNO | MB_ICONINFORMATION) != IDYES {
        return;
    }
    install_in_background(window, release);
}

/// Downloads, checks and starts the update on another thread; the installer closes this copy and opens the new one.
fn install_in_background(window: HWND, release: Release) {
    let target = window.0 as isize;
    thread::spawn(move || {
        let result = update::install(&release);
        let report = Box::new((release, result));
        unsafe {
            let _ = PostMessageW(Some(HWND(target as _)), WM_INSTALLED, WPARAM(0), LPARAM(Box::into_raw(report) as isize));
        }
    });
}

fn install_failed(window: HWND, release: &Release, error: &str) {
    log!("update failed: {error}");
    let text = fill_in(&t("Couldn't install the update.\n\n{}\n\nOpen the download page instead?"), &[error]);
    if message(window, &text, MB_YESNO | MB_ICONWARNING) == IDYES {
        update::open(&release.page);
    }
}

fn message(window: HWND, text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe { MessageBoxW(Some(window), PCWSTR(text.as_ptr()), TITLE, style | MB_SETFOREGROUND | MB_TOPMOST) }
}

/// A notification from the tray icon.
fn notify(window: HWND, title: &str, text: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    for (slot, unit) in data.szInfoTitle.iter_mut().zip(title.encode_utf16().take(63)) {
        *slot = unit;
    }
    for (slot, unit) in data.szInfo.iter_mut().zip(text.encode_utf16().take(255)) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn quit(window: HWND) {
    settings::close();
    remove_tray_icon(window);
    for i in 0..SHORTCUTS.len() {
        unsafe {
            let _ = UnregisterHotKey(Some(window), HOTKEY_BASE + i as i32);
        }
    }
    if let Some(mut app) = APP.with_borrow_mut(Option::take) {
        send(&app, Command::Quit);
        if let Some(snow) = app.snow.take() {
            let _ = snow.join();
        }
        unsafe {
            let _ = DestroyIcon(app.icon);
        }
    }
    unsafe { PostQuitMessage(0) };
}

/// Puts a jack-o'-lantern in the tray while it is Halloween and wanted, a leaf while leaves fall, and so on, recoloured
/// whenever the taskbar turns light or dark.
fn refresh_icon() {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        let now = unsafe { GetLocalTime() };
        let (_, falling, _) = effective(app, now.wMonth as u32);
        let emblem = Emblem::of(&app.settings, &falling);
        let light = light_taskbar();
        if emblem == app.emblem && light == app.light {
            return;
        }
        app.emblem = emblem;
        app.light = light;
        let icon = tray_icon(emblem, light);
        let data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: app.window, uID: 1, uFlags: NIF_ICON, hIcon: icon, ..Default::default() };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            let _ = DestroyIcon(app.icon);
        }
        app.icon = icon;
    });
}

/// The tray's icon at its size for the display's scale, or when its picture can't be had, the app icon from the
/// executable's resources.
fn tray_icon(emblem: Emblem, light: bool) -> HICON {
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
        if let Some(icon) = picture_icon(emblem.glyph(light), size) {
            return icon;
        }
        let Ok(instance) = GetModuleHandleW(None) else { return HICON::default() };
        LoadImageW(Some(instance.into()), PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map_or(HICON::default(), |icon| HICON(icon.0))
    }
}

/// The picture of `pictures` closest above `size` pixels across, or the largest, as an icon that size.
fn picture_icon(pictures: &[(i32, &[u8])], size: i32) -> Option<HICON> {
    let (_, picture) = pictures.iter().find(|(side, _)| *side >= size).or(pictures.last())?;
    // Icon resources have kept the version Windows 3 gave them; a PNG is taken as one.
    unsafe { CreateIconFromResourceEx(picture, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }.ok()
}

fn add_tray_icon(window: HWND, icon: HICON) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip("DesktopFlora".encode_utf16()) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
}

fn remove_tray_icon(window: HWND) {
    let data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: window, uID: 1, ..Default::default() };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}
