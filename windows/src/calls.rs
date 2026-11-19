//! Whether a call or a recording is under way: some program other than DesktopFlora is using the microphone or the
//! camera right now. Windows keeps, for every program that asked for them, when it last started and stopped using
//! them; one that has started and not yet stopped is using them now. This is what lights the microphone and camera
//! icons in the taskbar, and it needs no permission to read.

use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE, RRF_RT_REG_QWORD, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::core::{PCWSTR, PWSTR, w};

const STORES: [PCWSTR; 2] = [
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone"),
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\webcam"),
];

/// Whether any program is using the microphone or camera now.
pub fn under_way() -> bool {
    STORES.iter().any(|&store| in_use(store))
}

/// Looks through a store's programs: Store apps directly under it, desktop programs under `NonPackaged`.
fn in_use(store: PCWSTR) -> bool {
    let Some(key) = open(HKEY_CURRENT_USER, store) else { return false };
    let found = any_using(key);
    unsafe {
        let _ = RegCloseKey(key);
    }
    found
}

fn any_using(key: HKEY) -> bool {
    let mut index = 0;
    loop {
        let mut name = [0u16; 512];
        let mut length = name.len() as u32;
        let status = unsafe { RegEnumKeyExW(key, index, Some(PWSTR(name.as_mut_ptr())), &mut length, None, None, None, None) };
        if status == ERROR_NO_MORE_ITEMS || status != ERROR_SUCCESS {
            return false;
        }
        index += 1;
        let child_name: Vec<u16> = name[..length as usize].iter().copied().chain([0]).collect();
        let label = String::from_utf16_lossy(&name[..length as usize]);
        if label.to_lowercase().contains("desktopflora") {
            continue;
        }
        let Some(child) = open(key, PCWSTR(child_name.as_ptr())) else { continue };
        let using = if label == "NonPackaged" { any_using(child) } else { using_now(child) };
        unsafe {
            let _ = RegCloseKey(child);
        }
        if using {
            return true;
        }
    }
}

/// Started and not stopped since.
fn using_now(key: HKEY) -> bool {
    let time = |name: PCWSTR| {
        let mut value = 0u64;
        let mut size = 8u32;
        let mut kind = REG_VALUE_TYPE::default();
        let status = unsafe { RegGetValueW(key, PCWSTR::null(), name, RRF_RT_REG_QWORD, Some(&mut kind), Some((&raw mut value).cast()), Some(&mut size)) };
        (status == ERROR_SUCCESS).then_some(value)
    };
    time(w!("LastUsedTimeStart")).is_some_and(|start| start > 0) && time(w!("LastUsedTimeStop")) == Some(0)
}

fn open(parent: HKEY, path: PCWSTR) -> Option<HKEY> {
    let mut key = HKEY::default();
    (unsafe { RegOpenKeyExW(parent, path, None, KEY_READ, &mut key) } == ERROR_SUCCESS).then_some(key)
}
