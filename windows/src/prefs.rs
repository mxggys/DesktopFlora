use std::ffi::c_void;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
use windows::core::{BOOL, PCWSTR, w};

use snowhush_core::halloween::Choice;
use snowhush_core::simulation::{Look, Mode};

const KEY: PCWSTR = w!("Software\\Snowhush");
/// What falls by the number the registry keeps it as: new kinds only ever go on the end.
const MODES: [Mode; 11] = Mode::ALL;

/// Size and breeze choices, in the panel's order.
pub const SIZES: [(&str, f32); 3] = [("Small", 0.7), ("Normal", 1.0), ("Large", 1.45)];
pub const WINDS: [(&str, f32); 3] = [("Still", 0.25), ("Breezy", 1.0), ("Stormy", 3.0)];
/// When the snow shows: hours from and to, local time; the same hour twice means always.
pub const SCHEDULES: [(&str, u32, u32); 4] = [("Always", 0, 0), ("Evenings", 17, 23), ("Daytime", 8, 17), ("Nights", 20, 6)];
/// Room for colours of one's own.
pub const MAX_COLORS: usize = 5;
/// Eco draws at most this many frames a second.
const ECO_RATE: u32 = 20;

/// A keyboard shortcut: `MOD_*` flags as `RegisterHotKey` takes them, and a virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub modifiers: u32,
    pub key: u32,
}

const CTRL_ALT: u32 = 0x0002 | 0x0001;
/// What each shortcut does, where it is kept, and what it is until someone changes it.
pub const SHORTCUTS: [(&str, PCWSTR, Hotkey); 3] = [
    ("Snow on/off", w!("hotkeyToggle"), Hotkey { modifiers: CTRL_ALT, key: b'S' as u32 }),
    ("Clear snow", w!("hotkeyClear"), Hotkey { modifiers: CTRL_ALT, key: b'C' as u32 }),
    ("Next weather", w!("hotkeyWeather"), Hotkey { modifiers: CTRL_ALT, key: b'W' as u32 }),
];
const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Snowhush");

#[derive(Clone, Copy)]
pub struct Settings {
    /// How much falls, in percent: 0 to 100 in steps of 5.
    pub intensity: u32,
    /// Snow and leaves settle along the bottom of the screen.
    pub ground: bool,
    pub melt: bool,
    pub snow_on_windows: bool,
    pub behind_windows: bool,
    /// The snow stays over full-screen apps instead of fading out of their way.
    pub over_full_screen: bool,
    /// The snow settles at the bottom of the screen, under the taskbar, for a taskbar made see-through, as by TranslucentTB.
    pub behind_taskbar: bool,
    pub show_in_screenshots: bool,
    /// The snow also settles on what the desktop draws without a window of its own: a selection dragged across it.
    pub deep_interaction: bool,
    pub welcomed: bool,
    /// Unix seconds of the first run, or 0 for an install older than the stamp.
    pub installed_at: u32,
    pub halloween: Choice,
    /// What falls: snow, leaves or rain; the kind picked last.
    pub mode: Mode,
    /// Two kinds fall together: `mix`, the one picked last after the one picked before it.
    pub mixed: bool,
    pub mix: [Mode; 2],
    /// Drawn at the display's highest rate all the time, instead of only while something is pushed around.
    pub max_performance: bool,
    /// How opaque what falls is drawn, in percent: 10 to 100 in steps of 5.
    pub opacity: u32,
    /// Switched off: nothing falls and nothing shows, whatever else is chosen.
    pub paused: bool,
    /// What falls follows the sky outside, or the season where the person lives, instead of `mode`.
    pub local_weather: bool,
    pub seasonal: bool,
    /// Indices into `SIZES` and `WINDS`.
    pub size: usize,
    pub wind: usize,
    /// Snow and stars fall in the palette's colours instead of their own.
    pub custom_colors: bool,
    pub palette: [[f32; 3]; MAX_COLORS],
    pub palette_len: usize,
    /// An index into `SCHEDULES`.
    pub schedule: usize,
    /// One per entry of `SHORTCUTS`; `None` when someone removed it.
    pub hotkeys: [Option<Hotkey>; 3],
    pub fade_while_typing: bool,
    /// At most `ECO_RATE` frames a second.
    pub eco: bool,
}

impl Settings {
    /// Whether snow settles anywhere.
    pub fn accumulates(&self) -> bool {
        self.ground || self.snow_on_windows
    }

    pub fn load() -> Settings {
        let calm = !animations_enabled();
        // Before the slider, the intensity was one of Off, Light, Steady, Heavy and Variable.
        let stop = read(w!("snowfall")).map(|i| i.min(4) as usize);
        let welcomed = read(w!("welcomed")).is_some_and(|v| v != 0);
        let installed_at = read(w!("installedAt")).unwrap_or_else(|| {
            let stamp = if welcomed { 0 } else { now() };
            write(w!("installedAt"), stamp);
            stamp
        });
        // DesktopFlora before it was based on Snowhush's own modes kept what fell as `weather`.
        let weather = read(w!("weather")).and_then(|i| [Mode::Snow, Mode::Rain, Mode::Blossom, Mode::Autumn, Mode::Stars].get(i as usize).copied());
        let mode = read(w!("mode")).and_then(|i| MODES.get(i as usize).copied()).or(weather).unwrap_or(if read(w!("autumn")).is_some_and(|v| v != 0) {
            Mode::Autumn
        } else {
            Mode::Snow
        });
        let mut palette = [[1.0; 3]; MAX_COLORS];
        let mut palette_len = 0;
        for hex in read_text(w!("palette")).unwrap_or_default().split('|').take(MAX_COLORS) {
            let channel = |at: usize| hex.get(at..at + 2).and_then(|c| u8::from_str_radix(c, 16).ok()).map(|v| v as f32 / 255.0);
            if let (Some(r), Some(g), Some(b)) = (channel(0), channel(2), channel(4)) {
                palette[palette_len] = [r, g, b];
                palette_len += 1;
            }
        }
        let index = |name: PCWSTR, default: usize, count: usize| read(name).map_or(default, |i| i as usize).min(count - 1);
        // Before Accumulate picked where snow settles, it turned settling off everywhere at once.
        let settled = read(w!("accumulate")).is_none_or(|v| v != 0) || read(w!("settleGround")).is_some();
        Settings {
            intensity: read(w!("intensity")).map(|v| v.clamp(0, 100)).or(stop.map(|i| [0, 25, 50, 75, 50][i])).unwrap_or(if calm { 25 } else { 50 }),
            ground: read(w!("settleGround")).map_or(settled, |v| v != 0),
            melt: read(w!("meltSnow")).is_none_or(|v| v != 0),
            snow_on_windows: settled && read(w!("snowOnWindows")).is_none_or(|v| v != 0),
            behind_windows: read(w!("behindWindows")).is_some_and(|v| v != 0),
            over_full_screen: read(w!("showOverFullScreen")).is_some_and(|v| v != 0),
            behind_taskbar: read(w!("behindTaskbar")).is_some_and(|v| v != 0),
            show_in_screenshots: read(w!("showInScreenshots")).is_some_and(|v| v != 0),
            deep_interaction: read(w!("deepInteraction")).is_some_and(|v| v != 0),
            welcomed,
            installed_at,
            halloween: Choice { on: read(w!("halloweenOn")).is_some_and(|v| v != 0), skipped: read(w!("halloweenSkipped")).unwrap_or(0) },
            // Before rain there was only a switch for leaves.
            max_performance: read(w!("performance")).is_some_and(|v| v == 1),
            opacity: read(w!("fallingOpacity")).map_or(100, |v| v.clamp(10, 100)),
            mode,
            mixed: read(w!("mixed")).is_some_and(|v| v != 0),
            mix: mix_of(read(w!("mix0")), read(w!("mix1")), mode),
            paused: read(w!("paused")).is_some_and(|v| v != 0),
            local_weather: read(w!("localWeather")).is_some_and(|v| v != 0),
            seasonal: read(w!("seasonal")).is_some_and(|v| v != 0),
            size: index(w!("flakeSize"), 1, SIZES.len()),
            wind: index(w!("wind"), 1, WINDS.len()),
            custom_colors: read(w!("customColors")).is_some_and(|v| v != 0),
            palette,
            palette_len,
            schedule: index(w!("schedule"), 0, SCHEDULES.len()),
            hotkeys: SHORTCUTS.map(|(_, name, default)| match read(name) {
                None => Some(default),
                Some(0) => None,
                Some(packed) => Some(Hotkey { modifiers: packed >> 16, key: packed & 0xffff }),
            }),
            fade_while_typing: read(w!("fadeWhileTyping")).is_some_and(|v| v != 0),
            eco: read(w!("eco")).is_some_and(|v| v != 0),
        }
    }

    pub fn save(&self) {
        write(w!("intensity"), self.intensity);
        write(w!("settleGround"), self.ground as u32);
        write(w!("meltSnow"), self.melt as u32);
        write(w!("snowOnWindows"), self.snow_on_windows as u32);
        write(w!("behindWindows"), self.behind_windows as u32);
        write(w!("showOverFullScreen"), self.over_full_screen as u32);
        write(w!("behindTaskbar"), self.behind_taskbar as u32);
        write(w!("showInScreenshots"), self.show_in_screenshots as u32);
        write(w!("deepInteraction"), self.deep_interaction as u32);
        write(w!("welcomed"), self.welcomed as u32);
        write(w!("installedAt"), self.installed_at);
        write(w!("halloweenOn"), self.halloween.on as u32);
        write(w!("halloweenSkipped"), self.halloween.skipped);
        write(w!("mode"), MODES.iter().position(|&m| m == self.mode).unwrap_or(0) as u32);
        write(w!("mixed"), self.mixed as u32);
        write(w!("mix0"), MODES.iter().position(|&m| m == self.mix[0]).unwrap_or(0) as u32);
        write(w!("mix1"), MODES.iter().position(|&m| m == self.mix[1]).unwrap_or(0) as u32);
        write(w!("autumn"), (self.mode == Mode::Autumn) as u32);
        write(w!("performance"), self.max_performance as u32);
        write(w!("fallingOpacity"), self.opacity);
        write(w!("paused"), self.paused as u32);
        write(w!("localWeather"), self.local_weather as u32);
        write(w!("seasonal"), self.seasonal as u32);
        write(w!("flakeSize"), self.size as u32);
        write(w!("wind"), self.wind as u32);
        write(w!("customColors"), self.custom_colors as u32);
        let palette: Vec<String> = self.palette[..self.palette_len.min(MAX_COLORS)]
            .iter()
            .map(|c| c.iter().map(|v| format!("{:02x}", (v * 255.0).round() as u8)).collect())
            .collect();
        write_text(w!("palette"), &palette.join("|"));
        write(w!("schedule"), self.schedule as u32);
        for ((_, name, _), hotkey) in SHORTCUTS.iter().zip(self.hotkeys) {
            write(*name, hotkey.map_or(0, |hotkey| hotkey.modifiers << 16 | hotkey.key));
        }
        write(w!("fadeWhileTyping"), self.fade_while_typing as u32);
        write(w!("eco"), self.eco as u32);
    }

    /// Size, breeze and colours, as the simulation takes them.
    pub fn look(&self) -> Look {
        let palette = if self.custom_colors { self.palette[..self.palette_len.min(MAX_COLORS)].to_vec() } else { Vec::new() };
        Look { size: SIZES[self.size].1, wind: WINDS[self.wind].1, palette }
    }

    /// The most frames a second: `ECO_RATE` for Eco, zero for no cap.
    pub fn rate_cap(&self) -> u32 {
        if self.eco { ECO_RATE } else { 0 }
    }

    /// Whether the schedule lets the snow show at this hour.
    pub fn scheduled(&self, hour: u32) -> bool {
        let (_, from, to) = SCHEDULES[self.schedule];
        if from == to {
            true
        } else if from < to {
            (from..to).contains(&hour)
        } else {
            hour >= from || hour < to
        }
    }

    /// The kinds falling: the two mixed, or the one picked.
    pub fn modes(&self) -> Vec<Mode> {
        if self.mixed { self.mix.to_vec() } else { vec![self.mode] }
    }

    /// Picks a kind; mixed, a third kind picked takes the place of the one picked before the other. Whether anything
    /// changed.
    pub fn pick(&mut self, mode: Mode) -> bool {
        if self.mixed {
            if self.mix.contains(&mode) {
                return false;
            }
            self.mix = [self.mix[1], mode];
        } else if mode == self.mode {
            return false;
        }
        self.mode = mode;
        true
    }

    /// Mixed, the kind picked last falls along with the one picked before it; unmixed, it falls alone.
    pub fn toggle_mixed(&mut self) {
        self.mixed = !self.mixed;
        if self.mixed {
            let other = if self.mix.contains(&self.mode) { self.mix.iter().copied().find(|&m| m != self.mode) } else { Some(self.mix[1]) };
            self.mix = [other.unwrap_or(other_than(self.mode)), self.mode];
        }
    }
}

/// Two different kinds read from the settings, or `mode` and another when they don't make a pair.
fn mix_of(first: Option<u32>, second: Option<u32>, mode: Mode) -> [Mode; 2] {
    let kind = |i: Option<u32>| i.and_then(|i| MODES.get(i as usize).copied());
    match (kind(first), kind(second)) {
        (Some(a), Some(b)) if a != b => [a, b],
        _ => [other_than(mode), mode],
    }
}

/// The kind to mix with `mode` until another is picked.
fn other_than(mode: Mode) -> Mode {
    if mode == Mode::Snow { Mode::Autumn } else { Mode::Snow }
}

pub fn opens_at_login() -> bool {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let status =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast::<c_void>()), Some(&mut size)) };
    if status != ERROR_SUCCESS {
        return false;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    command().is_some_and(|command| command[..command.len() - 1] == buffer[..length])
}

/// Turns Open at Login on the first time, once: switched off later, it stays off.
pub fn open_at_login_by_default() {
    if read(w!("loginDefaulted")).is_some() {
        return;
    }
    if set_opens_at_login(true) == ERROR_SUCCESS {
        write(w!("loginDefaulted"), 1);
    }
}

/// The version that ran before this one, once, if it differs: for saying an update has happened.
pub fn take_new_version(current: &str) -> Option<String> {
    let before = read_text(w!("lastVersion"));
    write_text(w!("lastVersion"), current);
    before.filter(|before| before != current && !before.is_empty())
}

/// Programs whose windows get no snow, by executable name in lowercase, such as `code.exe`.
pub fn excluded_apps() -> Vec<String> {
    read_text(w!("excludedApps"))
        .map(|text| text.split('|').filter(|name| !name.is_empty()).map(str::to_lowercase).collect())
        .unwrap_or_default()
}

pub fn set_excluded_apps(apps: &[String]) {
    write_text(w!("excludedApps"), &apps.join("|"));
}

fn read_text(name: PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 4096];
    let mut size = (buffer.len() * 2) as u32;
    let status =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, KEY, name, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast::<c_void>()), Some(&mut size)) };
    if status != ERROR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buffer[..length]))
}

fn write_text(name: PCWSTR, text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, KEY, name, REG_SZ.0, Some(text.as_ptr().cast()), (text.len() * 2) as u32);
    }
}

pub fn set_opens_at_login(on: bool) -> WIN32_ERROR {
    unsafe {
        if !on {
            return RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE);
        }
        let Some(command) = command() else { return WIN32_ERROR(1) };
        RegSetKeyValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE, REG_SZ.0, Some(command.as_ptr().cast()), (command.len() * 2) as u32)
    }
}

/// The quoted path of this executable, NUL-terminated.
fn command() -> Option<Vec<u16>> {
    let path = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", path.display()).encode_utf16().chain([0]).collect())
}

fn read(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let status =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, KEY, name, RRF_RT_REG_DWORD, None, Some((&raw mut value).cast::<c_void>()), Some(&mut size)) };
    (status == ERROR_SUCCESS).then_some(value)
}

fn now() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32)
}

fn write(name: PCWSTR, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, KEY, name, REG_DWORD.0, Some((&raw const value).cast::<c_void>()), 4);
    }
}

/// The "Animation effects" setting: off means the person prefers less motion.
pub fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let result = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&raw mut enabled).cast::<c_void>()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    result.is_err() || enabled.as_bool()
}
