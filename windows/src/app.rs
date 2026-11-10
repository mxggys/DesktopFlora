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
use crate::panel::{self, Action, Source, View};
use crate::prefs::{self, MAX_COLORS, SCHEDULES, SHORTCUTS, SIZES, Settings, WINDS};
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
const SPONSORS: &str = "https://github.com/sponsors/mbezruchko";
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
const DAY: u32 = 24 * 60 * 60 * 1000;
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
    /// The program last in front other than DesktopFlora and the shell, which the panel offers to keep the snow off.
    static LAST_APP: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The tray icon and its panel, and the system events that pause the snow, on the main thread;
/// the snow itself runs on its own thread so the panel and other modal loops never stall it.
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
                open_panel();
            } else if event == NIN_SELECT {
                // A click may be the first of a double-click: the panel waits until it's clear it isn't.
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
            open_panel();
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
            unsafe { SetTimer(Some(window), TIMER_UPDATES, DAY, None) };
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

/// What falls now and how much, in percent, and the breeze's share from outside: from the panel, the sky outside or the
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
    panel::refresh();
}

/// On or off: for the panel's switch, the double-click and the shortcut.
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

/// Registers the shortcuts set in the panel, noting which another program already holds.
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
                        log!("shortcut {} is taken by another program", panel::describe(*hotkey));
                    }
                    taken
                }
                None => false,
            };
        }
    });
}

fn open_panel() {
    panel::toggle(panel::Handlers { view: Box::new(view), act: Box::new(act) });
}

/// What the panel shows.
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

/// What a click in the panel does.
fn act(action: Action) {
    let window = APP.with_borrow(|app| app.as_ref().map(|app| app.window));
    let Some(window) = window else { return };
    match action {
        Action::Toggle => return toggle(),
        Action::Quit => return quit(window),
        Action::Updates => return check_for_updates(window, true),
        Action::Support => return update::open(SPONSORS),
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
            Action::AddColor(rgb) => {
                if settings.palette_len < MAX_COLORS {
                    settings.palette[settings.palette_len] = rgb;
                    settings.palette_len += 1;
                }
                None
            }
            Action::RemoveColor(i) => {
                if i < settings.palette_len {
                    settings.palette.copy_within(i + 1..settings.palette_len, i);
                    settings.palette_len -= 1;
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
    if IGNORED.contains(&class.as_str()) || class == "DesktopFloraPanel" {
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
            if fresh.is_some() {
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
    panel::close();
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
