//! Game icons and accent hues (ADR 0005). The store keeps one normalized PNG
//! per game; its dominant color tints the GUI unless the user picked one.

use crate::store::format_hash;
use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use sha2::{Digest, Sha256};
use std::io::Cursor;
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

    #[test]
    fn normalized_icons_fit_256_pixels() {
        let png = normalize(&solid([0, 0, 255, 255])).unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        assert_eq!(dominant_hue(&decoded), Some(240));
    }
}
