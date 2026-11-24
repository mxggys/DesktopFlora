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
    // Night is dimmer and bluer, and dawn and dusk glow warm, nearly rosy, as light does when the sun is low.
    let night = [0.66, 0.72, 0.9];
    let warm = [1.0, 0.88, 0.76];
    std::array::from_fn(|i| {
        let base = night[i] + (1.0 - night[i]) * day;
        base + (warm[i] - base) * golden * 0.75
    })
}

/// What a display shows behind its windows: a picture, and when it was last written, or a solid colour.
#[derive(Clone, PartialEq, Debug)]
pub enum Backdrop {
    /// A picture, when it was last written, how it is fitted to the display (Windows' `DWPOS_*`), and the colour
    /// around it where it doesn't cover the display.
    Picture(PathBuf, Option<SystemTime>, i32, u32),
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
                        let position = self.desktop.GetPosition().map_or(4, |position| position.0);
                        let around = self.desktop.GetBackgroundColor().map_or(0, |color| color.0);
                        Some(Backdrop::Picture(path, modified, position, around))
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
            Backdrop::Picture(path, ..) => self.average(path).ok()?,
            Backdrop::Color(color) => std::array::from_fn(|i| ((color >> (8 * i)) & 0xff) as f32 / 255.0),
        };
        let luminance = average[0] * 0.2126 + average[1] * 0.7152 + average[2] * 0.0722;
        let shade = 0.775 / luminance.max(0.04);
        Some([(average[0] * shade).min(1.0), (average[1] * shade).min(1.0), (average[2] * shade).min(1.0), 0.35])
    }

    /// The backdrop as a small picture of a display this many pixels wide and high, as Windows shows it there:
    /// filled, fitted, stretched, centred, tiled or spanned, with the background colour where the picture doesn't
    /// reach. A solid colour is a single pixel. Rows run top-down, four bytes a pixel, red first.
    pub fn image(&self, backdrop: &Backdrop, display: (u32, u32)) -> Option<(u32, u32, Vec<u8>)> {
        match backdrop {
            Backdrop::Color(color) => Some((1, 1, rgba(*color).to_vec())),
            Backdrop::Picture(path, _, position, around) => {
                self.shown(path, display, *position, *around).inspect_err(|error| log!("backdrop picture failed: {error}")).ok()
            }
        }
    }

    fn shown(&self, path: &PathBuf, display: (u32, u32), position: i32, around: u32) -> Result<(u32, u32, Vec<u8>)> {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let (display_width, display_height) = (display.0.max(1) as f32, display.1.max(1) as f32);
        unsafe {
            let decoder = self.imaging.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut width, mut height) = (0u32, 0u32);
            frame.GetSize(&mut width, &mut height)?;
            let (width, height) = (width.max(1), height.max(1));
            // The picture, decoded small enough to look up quickly, large enough to show what a drop magnifies.
            let shrink = (768.0 / width.max(height) as f32).min(1.0);
            let (small_width, small_height) = (((width as f32 * shrink) as u32).max(1), ((height as f32 * shrink) as u32).max(1));
            let scaler = self.imaging.CreateBitmapScaler()?;
            scaler.Initialize(&frame, small_width, small_height, WICBitmapInterpolationModeFant)?;
            let converter = self.imaging.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let mut source = vec![0u8; (small_width * small_height * 4) as usize];
            converter.CopyPixels(std::ptr::null(), small_width * 4, &mut source)?;

            let (w, h) = (width as f32, height as f32);
            // Where on the display a point of the picture's own pixels lands: scaled by `scale`, its top left at `at`.
            let (scale_x, scale_y, at_x, at_y, tiled) = match position {
                // Centred at its own size.
                0 => (1.0, 1.0, (display_width - w) / 2.0, (display_height - h) / 2.0, false),
                // Repeated at its own size from the top left.
                1 => (1.0, 1.0, 0.0, 0.0, true),
                // Stretched to the display, never mind its shape.
                2 => (display_width / w, display_height / h, 0.0, 0.0, false),
                // Fitted: all of it shows, bars of the background colour beside it.
                3 => {
                    let s = (display_width / w).min(display_height / h);
                    (s, s, (display_width - w * s) / 2.0, (display_height - h * s) / 2.0, false)
                }
                // Filled, and spanned, which fills each display with its share: scaled to cover, its middle kept.
                _ => {
                    let s = (display_width / w).max(display_height / h);
                    (s, s, (display_width - w * s) / 2.0, (display_height - h * s) / 2.0, false)
                }
            };
            let out_width = 192u32;
            let out_height = ((out_width as f32 * display_height / display_width).round() as u32).clamp(1, 512);
            let background = rgba(around);
            let mut pixels = Vec::with_capacity((out_width * out_height * 4) as usize);
            for oy in 0..out_height {
                for ox in 0..out_width {
                    let dx = (ox as f32 + 0.5) / out_width as f32 * display_width;
                    let dy = (oy as f32 + 0.5) / out_height as f32 * display_height;
                    let mut px = (dx - at_x) / scale_x;
                    let mut py = (dy - at_y) / scale_y;
                    if tiled {
                        px = px.rem_euclid(w);
                        py = py.rem_euclid(h);
                    }
                    if px < 0.0 || py < 0.0 || px >= w || py >= h {
                        pixels.extend_from_slice(&background);
                        continue;
                    }
                    let sx = ((px * shrink) as u32).min(small_width - 1);
                    let sy = ((py * shrink) as u32).min(small_height - 1);
                    let k = ((sy * small_width + sx) * 4) as usize;
                    pixels.extend_from_slice(&source[k..k + 4]);
                }
            }
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

/// A `COLORREF` as four bytes, red first, opaque.
fn rgba(color: u32) -> [u8; 4] {
    [(color & 0xff) as u8, ((color >> 8) & 0xff) as u8, ((color >> 16) & 0xff) as u8, 255]
}

/// Copies a string the shell allocated and frees it.
unsafe fn take(text: PWSTR) -> String {
    let copy = unsafe { text.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(text.0 as *const _)) };
    copy
}
