use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::SystemTime;

use windows::Win32::Foundation::{GENERIC_READ, RECT};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};
use windows::core::{PCWSTR, PWSTR, Result};

use snowhush_core::halloween;

use crate::log;

/// What the hour brings to the snow's light: daylight, and how far into Halloween night it is.
#[derive(Clone, Copy)]
pub struct Sky {
    pub daylight: [f32; 3],
    pub night: f32,
}

impl Sky {
    /// The sky now; `halloween` while it is Halloween and wanted.
    pub fn now(halloween: bool) -> Sky {
        Sky { daylight: daylight(hour()), night: halloween::night(halloween) }
    }
}

/// The local time of day in hours, or the hour being previewed.
fn hour() -> f32 {
    halloween::previewed_hour().unwrap_or_else(|| {
        let now = unsafe { GetLocalTime() };
        now.wHour as f32 + now.wMinute as f32 / 60.0
    })
}

/// The scene turns warmer at dawn and dusk, cooler and dimmer at night.
/// Clock-based rather than solar: close enough for mood, and needs no location.
fn daylight(hour: f32) -> [f32; 3] {
    let day = halloween::daylight_share(hour);
    let golden = ((-((hour - 7.6) / 1.1).powi(2)).exp() + (-((hour - 18.6) / 1.2).powi(2)).exp()).min(1.0);
    let night = [0.86, 0.89, 0.97];
    let warm = [1.0, 0.965, 0.915];
    std::array::from_fn(|i| {
        let base = night[i] + (1.0 - night[i]) * day;
        base + (warm[i] - base) * golden * 0.55
    })
}

/// What a display shows behind its windows: a picture, and when it was last written, or a solid colour.
#[derive(Clone, PartialEq, Debug)]
pub enum Backdrop {
    Picture(PathBuf, Option<SystemTime>),
    Color(u32),
}

/// Shadows in the snow pick up the colour of the desktop picture along the bottom of the screen.
pub struct Wallpapers {
    desktop: IDesktopWallpaper,
    imaging: IWICImagingFactory,
}

impl Wallpapers {
    pub fn new() -> Result<Wallpapers> {
        unsafe {
            Ok(Wallpapers {
                desktop: CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)?,
                imaging: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
            })
        }
    }

    /// The backdrop of the display with these bounds, in physical pixels.
    pub fn backdrop(&self, bounds: &RECT) -> Option<Backdrop> {
        unsafe {
            for index in 0..self.desktop.GetMonitorDevicePathCount().ok()? {
                let Ok(monitor) = self.desktop.GetMonitorDevicePathAt(index) else { continue };
                let id = PCWSTR(monitor.0);
                let matches = self.desktop.GetMonitorRECT(id).is_ok_and(|rect| rect == *bounds);
                let picture = if matches { self.desktop.GetWallpaper(id).ok().map(|path| take(path)) } else { None };
                CoTaskMemFree(Some(monitor.0 as *const _));
                if !matches {
                    continue;
                }
                return match picture.filter(|path| !path.is_empty()) {
                    Some(path) => {
                        let path = PathBuf::from(path);
                        let modified = std::fs::metadata(&path).and_then(|metadata| metadata.modified()).ok();
                        Some(Backdrop::Picture(path, modified))
                    }
                    None => self.desktop.GetBackgroundColor().ok().map(|color| Backdrop::Color(color.0)),
                };
            }
        }
        None
    }

    /// The backdrop's average colour along the bottom quarter, brought to the brightness of snow in shade.
    pub fn tint(&self, backdrop: &Backdrop) -> Option<[f32; 4]> {
        let average = match backdrop {
            Backdrop::Picture(path, _) => self.average(path).ok()?,
            Backdrop::Color(color) => std::array::from_fn(|i| ((color >> (8 * i)) & 0xff) as f32 / 255.0),
        };
        let luminance = average[0] * 0.2126 + average[1] * 0.7152 + average[2] * 0.0722;
        let shade = 0.775 / luminance.max(0.04);
        Some([(average[0] * shade).min(1.0), (average[1] * shade).min(1.0), (average[2] * shade).min(1.0), 0.35])
    }

    /// The backdrop as a small picture covering a display of this width to height, as Windows fills a display with
    /// it by default: scaled to cover and cut to fit, its middle kept. A solid colour is a single pixel. Rows run
    /// top-down, four bytes a pixel, red first.
    pub fn image(&self, backdrop: &Backdrop, aspect: f32) -> Option<(u32, u32, Vec<u8>)> {
        match backdrop {
            Backdrop::Color(color) => Some((1, 1, vec![(color & 0xff) as u8, ((color >> 8) & 0xff) as u8, ((color >> 16) & 0xff) as u8, 255])),
            Backdrop::Picture(path, _) => self.covering(path, aspect).inspect_err(|error| log!("backdrop picture failed: {error}")).ok(),
        }
    }

    fn covering(&self, path: &PathBuf, aspect: f32) -> Result<(u32, u32, Vec<u8>)> {
        use windows::Win32::Graphics::Imaging::WICRect;
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            let decoder = self.imaging.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut width, mut height) = (0u32, 0u32);
            frame.GetSize(&mut width, &mut height)?;
            let (width, height) = (width.max(1), height.max(1));
            let aspect = aspect.max(0.1);
            // The part of the picture the display shows.
            let (cut_width, cut_height) =
                if width as f32 / height as f32 > aspect { (((height as f32 * aspect) as u32).max(1), height) } else { (width, ((width as f32 / aspect) as u32).max(1)) };
            let clipper = self.imaging.CreateBitmapClipper()?;
            let rect = WICRect { X: ((width - cut_width) / 2) as i32, Y: ((height - cut_height) / 2) as i32, Width: cut_width as i32, Height: cut_height as i32 };
            clipper.Initialize(&frame, &rect)?;
            // Small: it only gives the light and colour around each flake and drop, and what a drop shows, blurred.
            let out_width = 192u32;
            let out_height = ((out_width as f32 / aspect).round() as u32).clamp(1, 512);
            let scaler = self.imaging.CreateBitmapScaler()?;
            scaler.Initialize(&clipper, out_width, out_height, WICBitmapInterpolationModeFant)?;
            let converter = self.imaging.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let mut pixels = vec![0u8; (out_width * out_height * 4) as usize];
            converter.CopyPixels(std::ptr::null(), out_width * 4, &mut pixels)?;
            Ok((out_width, out_height, pixels))
        }
    }

    fn average(&self, path: &PathBuf) -> Result<[f32; 3]> {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            let decoder = self.imaging.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&mut width, &mut height)?;
            let scale = (64.0 / width.max(height).max(1) as f32).min(1.0);
            let (width, height) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
            let scaler = self.imaging.CreateBitmapScaler()?;
            scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant)?;
            let converter = self.imaging.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;

            // Rows run top-down, so the bottom of the picture is at the end.
            let bottom = &pixels[(height * 3 / 4 * width * 4) as usize..];
            let count = (bottom.len() / 4).max(1) as f32;
            let mut sum = [0.0f32; 3];
            for pixel in bottom.chunks_exact(4) {
                for i in 0..3 {
                    sum[i] += pixel[i] as f32 / 255.0;
                }
            }
            Ok(sum.map(|channel| channel / count))
        }
    }
}

/// Copies a string the shell allocated and frees it.
unsafe fn take(text: PWSTR) -> String {
    let copy = unsafe { text.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(text.0 as *const _)) };
    copy
}
