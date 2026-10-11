//! Game icons and accent hues (ADR 0005). The store keeps one normalized PNG
//! per game; its dominant color tints the GUI unless the user picked one.

use crate::store::format_hash;
use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

/// Icons are stored to fit within this many pixels on each side.
pub const ICON_SIZE: u32 = 256;
/// Width of one bucket of the hue histogram, in degrees.
const BUCKET_DEGREES: f32 = 10.0;

/// Scales `image` to fit `ICON_SIZE` and encodes it as PNG.
pub fn normalize(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    image
        .resize(ICON_SIZE, ICON_SIZE, FilterType::Lanczos3)
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .context("Failed to encode the icon as PNG")?;
    Ok(png)
}

pub fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    image::load_from_memory(bytes).context("Failed to decode the icon")
}

/// Where an icon comes from.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    /// A Steam game's icon in the client's `librarycache`, or a file from its
    /// install when the client has none.
    Steam { appid: u64, local: &'a Path },
    /// An image, an `.ico` or an executable.
    File(&'a Path),
}

/// The icon `source` points to, as large as it comes (exe icons up to 256 px).
pub fn resolve(source: Source) -> Result<DynamicImage> {
    resolve_with(source, download)
}

/// `resolve` with `fetch` downloading a URL's bytes. A Steam icon cached under
/// its `community_icon` hash is replaced by the 184 px `_full` one when the
/// download works; otherwise (offline, or an older app without one) the
/// local 32 px file is used.
pub fn resolve_with(
    source: Source,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>>,
) -> Result<DynamicImage> {
    let path = match source {
        Source::Steam { appid, local } => {
            if let Some(hash) = steam_icon_hash(local)
                && let Ok(image) =
                    fetch(&steam_icon_url(appid, hash)).and_then(|bytes| decode(&bytes))
            {
                return Ok(image);
            }
            local
        }
        Source::File(path) => path,
    };
    let is_exe = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if is_exe {
        from_exe(path)
    } else {
        image::open(path).context("Failed to open image file")
    }
}

/// The `community_icon` hash naming a current Steam client's cached icon,
/// `appcache/librarycache/{appid}/<40 hex>.jpg`.
pub fn steam_icon_hash(path: &Path) -> Option<&str> {
    let stem = path.file_stem()?.to_str()?;
    let is_jpg = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"));
    (is_jpg && stem.len() == 40 && stem.bytes().all(|b| b.is_ascii_hexdigit())).then_some(stem)
}

/// The 184 px community icon (without `_full` it is 32 px).
fn steam_icon_url(appid: u64, hash: &str) -> String {
    format!(
        "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{appid}/{hash}_full.jpg"
    )
}

/// Downloads `url` with a short timeout.
pub fn download(url: &str) -> Result<Vec<u8>> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(10))
        .call()
        .with_context(|| format!("Failed to download {url}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 << 20)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to download {url}"))?;
    Ok(bytes)
}

/// The executable's own icon (its first icon group) at the largest size it
/// holds, read straight from its resources.
#[cfg(windows)]
pub fn from_exe(path: &Path) -> Result<DynamicImage> {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
    };
    use windows::core::HSTRING;

    // As a data file: the exe's code is never run or its imports resolved.
    let module = unsafe {
        LoadLibraryExW(
            &HSTRING::from(path),
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    }
    .with_context(|| format!("Failed to open {}", path.display()))?;
    let images = exe_resources::first_icon_images(module);
    unsafe {
        let _ = FreeLibrary(module);
    }
    largest_icon_image(&images?)
}

#[cfg(not(windows))]
pub fn from_exe(_: &Path) -> Result<DynamicImage> {
    anyhow::bail!("Executable icon extraction is only supported on Windows")
}

/// The largest (then deepest) of an icon's `RT_ICON` images that decodes.
/// Each is a PNG or a DIB (`BITMAPINFOHEADER` + pixels + AND mask). They're
/// decoded one by one rather than as one `.ico`: `image`'s ICO decoder picks
/// by color depth before size, and rejects the whole file over quirks real
/// exes have (non-RGBA PNGs, directory sizes that disagree with the image).
#[cfg_attr(not(windows), allow(dead_code))]
fn largest_icon_image(images: &[Vec<u8>]) -> Result<DynamicImage> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    let header = |data: &[u8]| -> Option<(u32, u16)> {
        if data.starts_with(PNG) {
            let width = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
            let height = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
            Some((width.saturating_mul(height), 32))
        } else {
            let width = i32::from_le_bytes(data.get(4..8)?.try_into().ok()?);
            // The height covers the color image and the AND mask.
            let height = i32::from_le_bytes(data.get(8..12)?.try_into().ok()?) / 2;
            let depth = u16::from_le_bytes(data.get(14..16)?.try_into().ok()?);
            let valid = (1..=256).contains(&width) && (1..=256).contains(&height);
            valid.then_some(((width * height) as u32, depth))
        }
    };
    let mut ranked: Vec<_> = images
        .iter()
        .filter_map(|data| Some((header(data)?, data)))
        .collect();
    ranked.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
    ranked
        .into_iter()
        .find_map(|(_, data)| {
            if data.starts_with(PNG) {
                return image::load_from_memory_with_format(data, ImageFormat::Png).ok();
            }
            // A one-entry `.ico` around the DIB; the decoder reads its size
            // and depth from the DIB header when the entry says 0.
            let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let width = i32::from_le_bytes(data[4..8].try_into().ok()?);
            let height = i32::from_le_bytes(data[8..12].try_into().ok()?) / 2;
            ico[6] = width as u8;
            ico[7] = height as u8;
            ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
            ico.extend_from_slice(&22u32.to_le_bytes());
            ico.extend_from_slice(data);
            image::load_from_memory_with_format(&ico, ImageFormat::Ico).ok()
        })
        .context("Failed to decode the executable's icon")
}

#[cfg(windows)]
mod exe_resources {
    use anyhow::{Context, Result, ensure};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        EnumResourceNamesW, FindResourceW, LoadResource, LockResource, SizeofResource,
    };
    use windows::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON};
    use windows::core::{BOOL, PCWSTR};

    /// A resource name: an integer id, or a NUL-terminated string.
    enum Name {
        Id(u16),
        Text(Vec<u16>),
    }

    impl Name {
        fn as_pcwstr(&self) -> PCWSTR {
            match self {
                // MAKEINTRESOURCE
                Name::Id(id) => PCWSTR(*id as usize as *const u16),
                Name::Text(text) => PCWSTR(text.as_ptr()),
            }
        }
    }

    /// The `RT_ICON` images of the module's first `RT_GROUP_ICON`.
    pub fn first_icon_images(module: HMODULE) -> Result<Vec<Vec<u8>>> {
        let mut first: Option<Name> = None;
        // Returns FALSE (and an error) once the callback stops it; `first` tells.
        let _ = unsafe {
            EnumResourceNamesW(
                Some(module),
                RT_GROUP_ICON,
                Some(take_first),
                &mut first as *mut Option<Name> as isize,
            )
        };
        let group_name = first.context("The executable has no icon")?;
        let group = resource(module, group_name.as_pcwstr(), RT_GROUP_ICON)
            .context("Failed to read the executable's icon group")?;
        // GRPICONDIR: reserved, type, count; then 14-byte GRPICONDIRENTRYs
        // ending in the RT_ICON id. Some groups are shorter than their count.
        ensure!(group.len() >= 6, "Malformed icon group");
        let count = u16::from_le_bytes([group[4], group[5]]) as usize;
        let images: Vec<Vec<u8>> = group[6..]
            .as_chunks::<14>()
            .0
            .iter()
            .take(count)
            .filter_map(|entry| {
                let id = u16::from_le_bytes([entry[12], entry[13]]);
                resource(module, Name::Id(id).as_pcwstr(), RT_ICON)
            })
            .collect();
        ensure!(!images.is_empty(), "The executable's icon has no images");
        Ok(images)
    }

    unsafe extern "system" fn take_first(
        _: HMODULE,
        _: PCWSTR,
        name: PCWSTR,
        first: isize,
    ) -> BOOL {
        let first = unsafe { &mut *(first as *mut Option<Name>) };
        // IS_INTRESOURCE; a string name only lives during the callback.
        *first = Some(if (name.0 as usize) >> 16 == 0 {
            Name::Id(name.0 as usize as u16)
        } else {
            let text = unsafe { name.as_wide() };
            Name::Text(text.iter().copied().chain([0]).collect())
        });
        false.into()
    }

    fn resource(module: HMODULE, name: PCWSTR, kind: PCWSTR) -> Option<Vec<u8>> {
        unsafe {
            let info = FindResourceW(Some(module), name, kind);
            if info.is_invalid() {
                return None;
            }
            let data = LockResource(LoadResource(Some(module), info).ok()?) as *const u8;
            if data.is_null() {
                return None;
            }
            let size = SizeofResource(Some(module), info) as usize;
            Some(std::slice::from_raw_parts(data, size).to_vec())
        }
    }
}

/// The `icon_sha256` naming these icon bytes.
pub fn sha256(png: &[u8]) -> String {
    format_hash(&Sha256::digest(png))
}

/// The hue (0–359) of the icon's dominant color, or `None` when it has no
/// usable color. The icon is downscaled to 32 px; transparent, near-black and
/// near-gray (including near-white) pixels are skipped; the rest vote into
/// 10° hue buckets weighted by saturation. The answer is the weighted mean
/// hue of the heaviest bucket (the first one on a tie).
pub fn dominant_hue(image: &DynamicImage) -> Option<u16> {
    let small = image.thumbnail(32, 32).to_rgba8();
    let buckets = (360.0 / BUCKET_DEGREES) as usize;
    let mut weight = vec![0.0f32; buckets];
    let mut hue_sum = vec![0.0f32; buckets];
    for pixel in small.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 128 {
            continue;
        }
        let Some((hue, saturation)) = hue_saturation(r, g, b) else {
            continue;
        };
        let bucket = ((hue / BUCKET_DEGREES) as usize).min(buckets - 1);
        weight[bucket] += saturation;
        hue_sum[bucket] += hue * saturation;
    }
    let (best, &total) =
        weight.iter().enumerate().fold(
            (0, &0.0),
            |best, item| if item.1 > best.1 { item } else { best },
        );
    (total > 0.0).then(|| (hue_sum[best] / total).round() as u16 % 360)
}

/// HSV hue (degrees) and saturation of a pixel with a usable color.
fn hue_saturation(r: u8, g: u8, b: u8) -> Option<(f32, f32)> {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let saturation = if max > 0.0 { delta / max } else { 0.0 };
    // Near-black pixels have unstable hues; near-gray ones have none.
    if max < 0.15 || saturation < 0.2 {
        return None;
    }
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    Some((hue, saturation))
}

/// The hue used when a game has no icon or no usable color:
/// `uuid.as_u128() % 360`, so every device picks the same one.
pub fn fallback_hue(uuid: Uuid) -> u16 {
    (uuid.as_u128() % 360) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba(color)))
    }

    #[test]
    fn colored_icons_yield_their_hue() {
        assert_eq!(dominant_hue(&solid([255, 0, 0, 255])), Some(0));
        assert_eq!(dominant_hue(&solid([0, 200, 0, 255])), Some(120));
        assert_eq!(dominant_hue(&solid([0, 0, 255, 255])), Some(240));
        assert_eq!(dominant_hue(&solid([255, 128, 0, 255])), Some(30));
    }

    #[test]
    fn the_dominant_color_wins_over_gray_and_transparent_pixels() {
        let mut image = RgbaImage::from_pixel(64, 64, Rgba([128, 128, 128, 255]));
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            if y < 16 {
                *pixel = Rgba([0, 0, 255, 255]);
            } else if y < 24 && x < 32 {
                *pixel = Rgba([255, 0, 0, 255]);
            } else if y >= 48 {
                *pixel = Rgba([0, 255, 0, 0]);
            }
        }
        assert_eq!(dominant_hue(&DynamicImage::ImageRgba8(image)), Some(240));
    }

    #[test]
    fn gray_black_white_and_transparent_icons_have_no_hue() {
        for color in [
            [128, 128, 128, 255],
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [20, 0, 0, 255],
            [255, 0, 0, 0],
        ] {
            assert_eq!(dominant_hue(&solid(color)), None, "{color:?}");
        }
    }

    #[test]
    fn fallback_hue_is_deterministic_and_in_range() {
        for _ in 0..100 {
            let uuid = Uuid::new_v4();
            assert_eq!(fallback_hue(uuid), fallback_hue(uuid));
            assert!(fallback_hue(uuid) < 360);
        }
        assert_eq!(fallback_hue(Uuid::from_u128(725)), 5);
    }

    fn jpg(size: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::new(size, size))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    const HASH: &str = "3eec57d783c54036166564e195039b20f600a52f";

    #[test]
    fn ico_files_decode_to_their_largest_size() {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        let frames: Vec<_> = [16, 48, 32]
            .into_iter()
            .map(|size| {
                let pixels = RgbaImage::from_pixel(size, size, Rgba([255, 0, 0, 255]));
                IcoFrame::as_png(&pixels, size, size, image::ExtendedColorType::Rgba8).unwrap()
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let ico = dir.path().join("game.ico");
        IcoEncoder::new(std::fs::File::create(&ico).unwrap())
            .encode_images(&frames)
            .unwrap();
        let image = resolve(Source::File(&ico)).unwrap();
        assert_eq!((image.width(), image.height()), (48, 48));
    }

    #[test]
    fn steam_icons_are_downloaded_at_184_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(format!("{HASH}.jpg"));
        std::fs::write(&local, jpg(32)).unwrap();
        let mut requested = None;
        let image = resolve_with(
            Source::Steam {
                appid: 1029780,
                local: &local,
            },
            |url| {
                requested = Some(url.to_owned());
                Ok(jpg(184))
            },
        )
        .unwrap();
        assert_eq!(image.width(), 184);
        assert_eq!(
            requested.unwrap(),
            format!(
                "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/1029780/{HASH}_full.jpg"
            )
        );
    }

    #[test]
    fn a_failed_steam_download_falls_back_to_the_local_icon() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(format!("{HASH}.jpg"));
        std::fs::write(&local, jpg(32)).unwrap();
        let offline = |_: &str| anyhow::bail!("offline");
        let image = resolve_with(
            Source::Steam {
                appid: 1,
                local: &local,
            },
            offline,
        )
        .unwrap();
        assert_eq!(image.width(), 32);
        let garbage = |_: &str| Ok(b"<html>".to_vec());
        let image = resolve_with(
            Source::Steam {
                appid: 1,
                local: &local,
            },
            garbage,
        )
        .unwrap();
        assert_eq!(image.width(), 32);
    }

    #[test]
    fn legacy_steam_icons_have_no_hash_to_download() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("1029780_icon.jpg");
        std::fs::write(&local, jpg(32)).unwrap();
        let image = resolve_with(
            Source::Steam {
                appid: 1029780,
                local: &local,
            },
            |_| panic!("no download without a hash"),
        )
        .unwrap();
        assert_eq!(image.width(), 32);
    }

    #[test]
    fn only_40_hex_jpgs_name_a_community_icon() {
        let hash = |name: &str| steam_icon_hash(Path::new(name)).map(str::to_owned);
        assert_eq!(hash(&format!("x/{HASH}.jpg")), Some(HASH.to_owned()));
        assert_eq!(hash(&format!("x/{HASH}")), None);
        assert_eq!(hash(&format!("x/{HASH}.png")), None);
        assert_eq!(hash("x/header.jpg"), None);
        assert_eq!(hash(&format!("x/{}.jpg", &HASH[1..])), None);
        assert_eq!(hash(&format!("x/{}g.jpg", &HASH[1..])), None);
    }

    /// A 32 bpp `RT_ICON` DIB: header, pixels, 1 bpp AND mask.
    fn dib(size: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&40u32.to_le_bytes());
        data.extend_from_slice(&(size as i32).to_le_bytes());
        data.extend_from_slice(&(2 * size as i32).to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&32u16.to_le_bytes());
        data.resize(40, 0);
        data.resize(40 + (size * size * 4) as usize, 0xff);
        data.resize(data.len() + (size.div_ceil(32) * 4 * size) as usize, 0);
        data
    }

    #[test]
    fn the_largest_exe_icon_image_that_decodes_wins() {
        // RGB, which `image`'s ICO decoder would reject inside an `.ico`.
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::new(64, 64))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let mut broken = png.clone();
        broken[16..24].copy_from_slice(&[0, 0, 1, 0, 0, 0, 1, 0]);
        broken.truncate(40);

        let image = largest_icon_image(&[dib(16), png.clone(), dib(48)]).unwrap();
        assert_eq!(image.width(), 64);
        let image = largest_icon_image(&[dib(16), broken.clone(), dib(48)]).unwrap();
        assert_eq!(image.width(), 48);
        assert!(largest_icon_image(&[broken]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn exe_icons_come_out_larger_than_32_pixels() {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let image = from_exe(&Path::new(&windir).join("explorer.exe")).unwrap();
        assert!(image.width() > 32, "{}px", image.width());
    }

    #[test]
    fn normalized_icons_fit_256_pixels() {
        let png = normalize(&solid([0, 0, 255, 255])).unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        assert_eq!(dominant_hue(&decoded), Some(240));
    }
}
