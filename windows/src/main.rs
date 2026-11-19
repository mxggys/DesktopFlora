#![windows_subsystem = "windows"]

mod app;
mod desktop;
mod halloween;
mod light;
mod log;
mod outside;
mod menu;
mod prefs;
mod render;
mod screen;
mod settings;
mod snow;
mod text;
mod tracker;
mod update;
mod welcome;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::core::w;

fn main() {
    // What "Uninstall" in Settings runs; it removes everything, even with Snowhush still running.
    if std::env::args().skip(1).any(|argument| argument == "--uninstall") {
        snowhush::install::uninstall();
        return;
    }
    unsafe {
        let _instance = CreateMutexW(None, true, w!("Local\\DesktopFlora"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    prefs::migrate();
    snowhush::language::choose();
    log::start();
    std::panic::set_hook(Box::new(|info| log!("panic on the {} thread: {info}", std::thread::current().name().unwrap_or("unnamed"))));
    log!("DesktopFlora {}", env!("CARGO_PKG_VERSION"));
    if let Err(error) = app::run() {
        log!("failed: {error}");
    }
}
