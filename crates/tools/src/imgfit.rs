//! Downscaling and recompressing images before they reach the model, modeled on oh-my-pi's
//! `resizeImage`: fit the longest edge into 1568px (the size Claude scales images to anyway),
//! aim for 500KB, lift tiny images to a 200px short edge, and pick the smaller of PNG and JPEG.
//! Images with translucent pixels stay PNG (JPEG would show the color under the transparency).

use base64::Engine as _;
use fast_image_resize::{Resizer, images::Image};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};
use ref_proto::ImageData;
use std::io::Cursor;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_width: u32,
    pub max_height: u32,
    /// Target size of the encoded image.
    pub max_bytes: usize,
    /// JPEG quality of the first attempt; the fallback ladder goes lower.
    pub jpeg_quality: u8,
    /// Smallest wanted edge; smaller images are scaled up uniformly as far as the caps allow.
    pub min_dimension: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_width: 1568,
            max_height: 1568,
            max_bytes: 500 * 1024,
            jpeg_quality: 80,
            min_dimension: 200,
        }
    }
}

/// Result of [`fit`].
#[derive(Debug, Clone)]
pub struct Fitted {
    /// Re-encoded image, or `None` when the original already fits and is kept as is.
    pub data: Option<Vec<u8>>,
    /// Mime type of `data` (of the original when `data` is `None`).
    pub mime: &'static str,
    pub original: (u32, u32),
    pub size: (u32, u32),
}

impl Fitted {
    /// Tells the model how coordinates map back when the dimensions changed.
    pub fn note(&self) -> Option<String> {
        let ((ow, oh), (w, h)) = (self.original, self.size);
        if self.data.is_none() || (ow, oh) == (w, h) || w == 0 {
            return None;
        }
        Some(format!(
            "[Image: original {ow}x{oh}, displayed at {w}x{h}. Multiply coordinates by {:.2} to map to original image.]",
            ow as f64 / w as f64
        ))
    }
}

const QUALITY_STEPS: [u8; 4] = [70, 60, 50, 40];
const SCALE_STEPS: [f64; 4] = [0.75, 0.5, 0.35, 0.25];

fn mime_of(f: ImageFormat) -> Option<&'static str> {
    Some(match f {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::WebP => "image/webp",
        _ => return None,
    })
}

/// Format, mime and dimensions from the header, without decoding the pixels.
fn probe(bytes: &[u8]) -> Result<(ImageFormat, &'static str, u32, u32), String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let format = reader.format().ok_or("unknown image format")?;
    let mime = mime_of(format).ok_or_else(|| format!("unsupported image format {format:?}"))?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 {
        return Err("image has no pixels".into());
    }
    Ok((format, mime, w, h))
}

/// Decodes to RGB8, or RGBA8 when some pixel is translucent.
fn decode(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage, String> {
    let img = image::load_from_memory_with_format(bytes, format).map_err(|e| e.to_string())?;
    Ok(if img.color().has_alpha() {
        let rgba = img.into_rgba8();
        if rgba.pixels().any(|p| p[3] < 255) {
            DynamicImage::ImageRgba8(rgba)
        } else {
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(rgba).into_rgb8())
        }
    } else {
        DynamicImage::ImageRgb8(img.into_rgb8())
    })
}

/// Fits `bytes` (png, jpeg, gif or webp) into `limits`. Errors if the image cannot be decoded.
/// Animated GIFs that need re-encoding keep only their first frame.
pub fn fit(bytes: &[u8], limits: &Limits) -> Result<Fitted, String> {
    let (format, mime, ow, oh) = probe(bytes)?;
    let (max_w, max_h) = (limits.max_width.max(1), limits.max_height.max(1));
    let min_dim = limits.min_dimension.min(max_w).min(max_h);
    // Already within the bounds and compact (a quarter of the budget): keep the original bytes.
    if ow >= min_dim
        && oh >= min_dim
        && ow <= max_w
        && oh <= max_h
        && bytes.len() <= limits.max_bytes / 4
    {
        return Ok(Fitted {
            data: None,
            mime,
            original: (ow, oh),
            size: (ow, oh),
        });
    }

    let (mut tw, mut th) = (ow as f64, oh as f64);
    if tw > max_w as f64 {
        th = (th * max_w as f64 / tw).round();
        tw = max_w as f64;
    }
    if th > max_h as f64 {
        tw = (tw * max_h as f64 / th).round();
        th = max_h as f64;
    }
    // Lift undersized images toward the minimum without crossing a cap or distorting them.
    if tw < min_dim as f64 || th < min_dim as f64 {
        let up = (min_dim as f64 / tw.min(th))
            .min(max_w as f64 / tw)
            .min(max_h as f64 / th);
        if up > 1.0 {
            tw = (tw * up).round().min(max_w as f64);
            th = (th * up).round().min(max_h as f64);
        }
    }
    let (tw, th) = ((tw as u32).max(1), (th as u32).max(1));

    let src = decode(bytes, format)?;
    // JPEG has no alpha; translucent images can only shrink by dimensions.
    let lossy = !src.color().has_alpha();

    let mut resizer = Resizer::new();
    let mut best: Option<Fitted> = None;
    let mut consider = |data: Vec<u8>, mime: &'static str, size: (u32, u32)| -> bool {
        let fits = data.len() <= limits.max_bytes;
        if best
            .as_ref()
            .is_none_or(|b| data.len() < b.data.as_ref().map_or(usize::MAX, Vec::len))
        {
            best = Some(Fitted {
                data: Some(data),
                mime,
                original: (ow, oh),
                size,
            });
        }
        fits
    };

    let target = resize(&mut resizer, &src, tw, th)?;
    let png = encode_png(&target)?;
    let jpeg = if lossy {
        Some(encode_jpeg(&target, limits.jpeg_quality)?)
    } else {
        None
    };
    let fits_png = consider(png, "image/png", (tw, th));
    let fits_jpeg = jpeg
        .map(|j| consider(j, "image/jpeg", (tw, th)))
        .unwrap_or(false);
    let mut done = fits_png || fits_jpeg;
    if !done && lossy {
        for q in QUALITY_STEPS {
            if consider(encode_jpeg(&target, q)?, "image/jpeg", (tw, th)) {
                done = true;
                break;
            }
        }
    }
    if !done {
        'scales: for scale in SCALE_STEPS {
            let (w, h) = (
                (tw as f64 * scale).round() as u32,
                (th as f64 * scale).round() as u32,
            );
            if w < 100 || h < 100 {
                break;
            }
            let small = resize(&mut resizer, &src, w, h)?;
            if !lossy {
                if consider(encode_png(&small)?, "image/png", (w, h)) {
                    break;
                }
                continue;
            }
            for q in QUALITY_STEPS {
                if consider(encode_jpeg(&small, q)?, "image/jpeg", (w, h)) {
                    break 'scales;
                }
            }
        }
    }
    let best = best.expect("at least one candidate");
    // Same dimensions and no smaller: re-encoding gained nothing.
    if best.size == (ow, oh) && best.data.as_ref().is_some_and(|d| d.len() >= bytes.len()) {
        return Ok(Fitted {
            data: None,
            mime,
            original: (ow, oh),
            size: (ow, oh),
        });
    }
    Ok(best)
}

fn resize(
    resizer: &mut Resizer,
    src: &DynamicImage,
    w: u32,
    h: u32,
) -> Result<DynamicImage, String> {
    if (src.width(), src.height()) == (w, h) {
        return Ok(src.clone());
    }
    let pixel =
        fast_image_resize::IntoImageView::pixel_type(src).ok_or("unsupported pixel layout")?;
    let mut dst = Image::new(w, h, pixel);
    // Lanczos3 with alpha premultiplied during the resize (the crate's defaults).
    resizer
        .resize(src, &mut dst, None)
        .map_err(|e| e.to_string())?;
    let buf = dst.into_vec();
    let out = if src.color().has_alpha() {
        image::RgbaImage::from_raw(w, h, buf).map(DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(w, h, buf).map(DynamicImage::ImageRgb8)
    };
    out.ok_or_else(|| "resize produced a wrong buffer size".into())
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_with_encoder(PngEncoder::new(&mut out))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// [`fit`] with default limits for a base64 image (pasted or attached by the user). Returns
/// the replacement, or `None` to keep the image (it already fits or cannot be decoded).
pub fn fit_image_data(img: &ImageData) -> Option<ImageData> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(img.data.as_bytes())
        .ok()?;
    let fitted = guarded(|| fit(&bytes, &Limits::default()))?;
    let data = fitted.data?;
    Some(ImageData::new(
        fitted.mime,
        base64::engine::general_purpose::STANDARD.encode(data),
    ))
}

/// Runs a decoder step, treating an error or a panic (malformed input) as no result.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .ok()?
        .ok()
}

/// Result of [`clamp`].
#[derive(Debug, Clone)]
pub struct Clamped {
    pub mime: &'static str,
    pub data: Vec<u8>,
    pub size: (u32, u32),
}

/// Base64 length of `n` bytes (what Claude's size limit measures).
fn b64_len(n: usize) -> usize {
    n.div_ceil(3) * 4
}

/// Re-encode passes allowed to bring one image under the byte budget in [`clamp`].
const CLAMP_ATTEMPTS: usize = 4;

/// Request-time cap, modeled on oh-my-pi's Anthropic image contract: an image with a side over
/// `max_dim` or a base64 size over `max_b64` is resized to fit (PNG or JPEG quality 85, the
/// smaller), shrinking by the overshoot for up to 4 passes. Returns `None` when the image already
/// fits or re-encoding would not make it smaller.
pub fn clamp(bytes: &[u8], max_dim: u32, max_b64: usize) -> Result<Option<Clamped>, String> {
    let (format, _, w, h) = probe(bytes)?;
    let max_dim = max_dim.max(1);
    let over_dim = w > max_dim || h > max_dim;
    let orig_b64 = b64_len(bytes.len());
    if !over_dim && orig_b64 <= max_b64 {
        return Ok(None);
    }
    let src = decode(bytes, format)?;
    let lossy = !src.color().has_alpha();
    let mut resizer = Resizer::new();
    let mut scale = (max_dim as f64 / w as f64)
        .min(max_dim as f64 / h as f64)
        .min(1.0);
    let mut best = None;
    for _ in 0..CLAMP_ATTEMPTS {
        let tw = ((w as f64 * scale).round() as u32).clamp(1, max_dim);
        let th = ((h as f64 * scale).round() as u32).clamp(1, max_dim);
        let img = resize(&mut resizer, &src, tw, th)?;
        let png = encode_png(&img)?;
        let (mime, data) = match lossy.then(|| encode_jpeg(&img, 85)).transpose()? {
            Some(j) if j.len() < png.len() => ("image/jpeg", j),
            _ => ("image/png", png),
        };
        let enc = b64_len(data.len());
        best = Some(Clamped {
            mime,
            data,
            size: (tw, th),
        });
        if enc <= max_b64 || (tw, th) == (1, 1) {
            break;
        }
        // Fitting the pixel cap does not imply fitting the byte cap: shrink by the overshoot in area.
        scale *= ((max_b64 as f64 / enc as f64).sqrt() * 0.9).max(0.25);
    }
    let best = best.expect("at least one pass");
    if !over_dim && b64_len(best.data.len()) >= orig_b64 {
        return Ok(None);
    }
    Ok(Some(best))
}

/// Recent [`clamp_image_data`] results, so a long conversation does not re-encode the same
/// images on every request. Bounded by the bytes of the stored replacements and by count.
struct ClampCache {
    entries: std::collections::VecDeque<(u128, Option<ImageData>)>,
    bytes: usize,
}

static CLAMP_CACHE: std::sync::Mutex<ClampCache> = std::sync::Mutex::new(ClampCache {
    entries: std::collections::VecDeque::new(),
    bytes: 0,
});
const CLAMP_CACHE_BYTES: usize = 64 << 20;
const CLAMP_CACHE_ENTRIES: usize = 1024;

/// [`clamp`] for a base64 image, cached. `None` keeps the image (it fits, or cannot be decoded).
pub fn clamp_image_data(img: &ImageData, max_dim: u32, max_b64: usize) -> Option<ImageData> {
    if img.is_stub() {
        return None;
    }
    let key = xxhash_rust::xxh3::xxh3_128_with_seed(
        img.data.as_bytes(),
        ((max_dim as u64) << 40) ^ max_b64 as u64,
    );
    if let Some((_, hit)) = CLAMP_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .iter()
        .find(|(k, _)| *k == key)
    {
        return hit.clone();
    }
    let out = base64::engine::general_purpose::STANDARD
        .decode(img.data.as_bytes())
        .ok()
        .and_then(|b| guarded(|| clamp(&b, max_dim, max_b64)).flatten())
        .map(|c| {
            ImageData::new(
                c.mime,
                base64::engine::general_purpose::STANDARD.encode(c.data),
            )
        });
    let mut c = CLAMP_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.bytes += out.as_ref().map_or(0, |i| i.data.len());
    c.entries.push_back((key, out.clone()));
    while c.bytes > CLAMP_CACHE_BYTES || c.entries.len() > CLAMP_CACHE_ENTRIES {
        let Some((_, old)) = c.entries.pop_front() else {
            break;
        };
        c.bytes -= old.map_or(0, |i| i.data.len());
    }
    out
}

/// Test images: noise is photo-like for JPEG (does not compress as PNG), flat compresses well.
#[cfg(test)]
pub(crate) fn test_png(w: u32, h: u32, noise: bool) -> Vec<u8> {
    let mut x: u32 = 12345;
    let img = image::RgbImage::from_fn(w, h, |px, py| {
        if !noise {
            return image::Rgb([10, 20, 30]);
        }
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        let n = (x >> 24) as u8 / 8;
        image::Rgb([(px % 256) as u8 / 2 + n, (py % 256) as u8 / 2 + n, 128 + n])
    });
    encode_png(&DynamicImage::ImageRgb8(img)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(img: &DynamicImage) -> Vec<u8> {
        encode_png(img).unwrap()
    }

    #[test]
    fn small_image_is_kept() {
        let bytes = test_png(400, 300, false);
        let f = fit(&bytes, &Limits::default()).unwrap();
        assert!(
            f.data.is_none() && f.mime == "image/png" && f.size == (400, 300) && f.note().is_none()
        );
    }

    #[test]
    fn large_image_is_downscaled_and_compressed() {
        let bytes = test_png(3000, 1000, true);
        let l = Limits::default();
        let f = fit(&bytes, &l).unwrap();
        let data = f.data.as_ref().unwrap();
        assert_eq!(f.original, (3000, 1000));
        assert!(f.size.0 <= 1568 && f.size.1 <= 1568, "{:?}", f.size);
        assert!(data.len() <= l.max_bytes, "{}", data.len());
        assert_eq!(f.mime, "image/jpeg");
        assert_eq!(image::load_from_memory(data).unwrap().width(), f.size.0);
        assert!(
            f.note()
                .unwrap()
                .starts_with("[Image: original 3000x1000, displayed at ")
        );
    }

    #[test]
    fn translucent_stays_png_and_tiny_is_lifted() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(20, 10, |x, _| {
            image::Rgba([255, 0, 0, if x < 10 { 0 } else { 255 }])
        }));
        let f = fit(&png(&img), &Limits::default()).unwrap();
        assert_eq!(f.mime, "image/png");
        assert_eq!(f.size, (400, 200));
        let out = image::load_from_memory(f.data.as_ref().unwrap())
            .unwrap()
            .into_rgba8();
        assert_eq!(out.get_pixel(5, 100)[3], 0);
        assert_eq!(*out.get_pixel(395, 100), image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn undecodable_is_an_error() {
        assert!(fit(b"\x89PNG\r\n\x1a\nbroken", &Limits::default()).is_err());
        let ok = ImageData::new("image/png", "iVBORw0KGgo=");
        assert!(fit_image_data(&ok).is_none());
    }

    #[test]
    fn clamp_dimensions_and_budget() {
        let big = test_png(3000, 1000, true);
        let c = clamp(&big, 2000, 10 << 20).unwrap().unwrap();
        assert_eq!((c.mime, c.size), ("image/jpeg", (2000, 667)));
        assert_eq!(image::load_from_memory(&c.data).unwrap().height(), 667);
        // Within the caps: untouched.
        assert!(clamp(&big, 8000, 10 << 20).unwrap().is_none());
        // Over the byte budget only: shrunk until the base64 fits.
        let c = clamp(&big, 8000, 100_000).unwrap().unwrap();
        assert!(
            b64_len(c.data.len()) <= 100_000 && c.size.0 < 3000,
            "{} {:?}",
            c.data.len(),
            c.size
        );
        // Cached result, same bytes; stubs and undecodable images are kept.
        let img = ImageData::new(
            "image/png",
            base64::engine::general_purpose::STANDARD.encode(&big),
        );
        let a = clamp_image_data(&img, 2000, 10 << 20).unwrap();
        let b = clamp_image_data(&img, 2000, 10 << 20).unwrap();
        assert!(a.mime == "image/jpeg" && a.data == b.data);
        assert!(clamp_image_data(&img, 8000, 10 << 20).is_none());
        assert!(clamp_image_data(&ImageData::new("image/png", ""), 1, 1).is_none());
        assert!(clamp_image_data(&ImageData::new("image/png", "iVBORw0KGgo="), 1, 1).is_none());
    }
}
