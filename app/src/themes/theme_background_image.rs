//! Normalising a background image on its way into a theme.
//!
//! A theme's background image used to be a byte-for-byte copy of whatever the user picked. That
//! meant a theme could carry a 7.7 megapixel screenshot where every bundled theme ships something
//! around 3 megapixels, and could carry an alpha channel that the renderer has to drag around for
//! no benefit. The picker then paid for that on every preview, and so did the window.
//!
//! Imported images are therefore decoded, flattened, capped and re-encoded to match the shape of
//! the built-in ones: JPEG, no alpha, longest edge no greater than [`MAX_EDGE`].

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader, RgbImage};

/// Longest edge kept for an imported background, in pixels.
///
/// Sits just above the largest bundled theme image (2356px wide, `pink_city_bg.jpg`) and covers a
/// 2x-DPI window around 1280pt across, so capping here costs nothing visible while roughly halving
/// the pixels the renderer touches for a typical phone or camera capture.
pub const MAX_EDGE: u32 = 2560;

/// Longest edge kept for the small preview written beside the full image.
///
/// The theme picker and explorer draw cards at roughly this size, and a card that decodes a 380px
/// JPEG instead of a 2560px one is the difference between a cheap paint and the multi-millisecond
/// decode-plus-resize that the bundled themes were given `jpg/thumbs/` to avoid. It also caps what
/// the asset cache keeps resident: theme-image sources are never evicted, so a full-res card would
/// hold ~15MB of decoded RGBA for the life of the process, per image theme.
pub const THUMB_EDGE: u32 = 380;

/// Quality passed to the JPEG encoder. Matches the perceptual ballpark of the bundled images,
/// which land between 420KB and 1MB at these dimensions.
const JPEG_QUALITY: u8 = 82;

/// Refuse inputs that are implausible as a wallpaper before allocating for them. A decoded 100MP
/// image would be 400MB of RGBA.
const MAX_INPUT_PIXELS: u64 = 100_000_000;

/// The extension every imported background ends up with, since the output is always JPEG.
pub const IMPORTED_EXTENSION: &str = "jpg";

/// The preview thumbnail that sits next to `<slug>.jpg`.
///
/// Named as a sibling with a `.thumb.jpg` suffix so it travels with the theme and is trivial to
/// derive from the full image's path without another lookup.
pub fn thumbnail_path(full_image: &Path) -> PathBuf {
    let stem = full_image
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    full_image.with_file_name(format!("{stem}.thumb.{IMPORTED_EXTENSION}"))
}

/// Decodes the file at `source`, normalises it, and writes it as `<dir>/<slug>.jpg` with a preview
/// thumbnail alongside. Returns the full image's path. The source file is never modified.
pub fn import(source: &Path, dir: &Path, slug: &str) -> Result<PathBuf> {
    let reader = ImageReader::open(source)
        .with_context(|| format!("couldn't open {}", source.display()))?
        // Trust the bytes rather than the extension: people rename files, and at least one image
        // shipped with the app is a PNG called `.jpg`.
        .with_guessed_format()
        .with_context(|| format!("couldn't read {}", source.display()))?;

    if let Some((w, h)) = reader.into_dimensions().ok().map(|d| (d.0 as u64, d.1 as u64)) {
        if w * h > MAX_INPUT_PIXELS {
            bail!("that image is {w}x{h}, which is too large to use as a background");
        }
    }

    // `into_dimensions` consumed the reader, so open it again now the size is known to be sane.
    let decoded = ImageReader::open(source)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("couldn't decode {}", source.display()))?;

    write_normalised(decoded, dir, slug)
}

/// The same as [`import`] but for an image already in memory — a background downloaded from the
/// gallery, which arrives as bytes rather than a file. Community images are normalised on the way
/// in exactly like imported ones, so a theme can't ship a 4K wallpaper that every card then pays to
/// decode.
pub fn import_bytes(bytes: &[u8], dir: &Path, slug: &str) -> Result<PathBuf> {
    let decoded = image::load_from_memory(bytes).context("couldn't decode the downloaded image")?;

    let (w, h) = (decoded.width() as u64, decoded.height() as u64);
    if w * h > MAX_INPUT_PIXELS {
        bail!("that image is {w}x{h}, which is too large to use as a background");
    }

    write_normalised(decoded, dir, slug)
}

/// Writes the capped full image and its preview thumbnail, and returns the full image's path.
fn write_normalised(decoded: DynamicImage, dir: &Path, slug: &str) -> Result<PathBuf> {
    let full_path = dir.join(format!("{slug}.{IMPORTED_EXTENSION}"));
    write_jpeg(&normalise(&decoded, MAX_EDGE), &full_path)?;

    // A best-effort thumbnail: a card without one still works — it falls back to the full image —
    // so a thumbnail failure must not sink the whole install.
    let _ = write_jpeg(&normalise(&decoded, THUMB_EDGE), &thumbnail_path(&full_path));

    Ok(full_path)
}

fn write_jpeg(image: &RgbImage, destination: &Path) -> Result<()> {
    let file = File::create(destination)
        .with_context(|| format!("couldn't create {}", destination.display()))?;
    JpegEncoder::new_with_quality(BufWriter::new(file), JPEG_QUALITY)
        .encode_image(image)
        .with_context(|| format!("couldn't encode {}", destination.display()))
}

/// Caps the longest edge and resolves any alpha channel.
///
/// Alpha has to go because the output is JPEG. It is composited onto black rather than simply
/// discarded — `DynamicImage::to_rgb8` drops the channel and keeps the raw colour underneath, so a
/// fully transparent pixel would come back as whatever happened to be stored there. Black is what
/// a transparent pixel would have shown anyway, since a background image sits at the very back of
/// the window with nothing painted behind it.
///
/// Images already within `max_edge` and already opaque pass through with only the colour-model
/// conversion. The caller decodes once and normalises to more than one size (full and thumbnail),
/// so this borrows rather than consumes.
fn normalise(image: &DynamicImage, max_edge: u32) -> RgbImage {
    let (width, height) = image.dimensions();
    let longest = width.max(height);

    let resized;
    let image: &DynamicImage = if longest > max_edge {
        let scale = max_edge as f32 / longest as f32;
        // Round up so neither edge can land on zero for an extreme aspect ratio.
        let target_w = ((width as f32 * scale).ceil() as u32).max(1);
        let target_h = ((height as f32 * scale).ceil() as u32).max(1);
        resized = image.resize_exact(target_w, target_h, FilterType::Lanczos3);
        &resized
    } else {
        image
    };

    if !image.color().has_alpha() {
        return image.to_rgb8();
    }

    let source = image.to_rgba8();
    let mut flattened = RgbImage::new(source.width(), source.height());
    for (to, from) in flattened.pixels_mut().zip(source.pixels()) {
        let alpha = from[3] as u32;
        // Straight alpha over black: out = src * a. Rounded, so a=255 is exactly lossless.
        *to = image::Rgb([
            ((from[0] as u32 * alpha + 127) / 255) as u8,
            ((from[1] as u32 * alpha + 127) / 255) as u8,
            ((from[2] as u32 * alpha + 127) / 255) as u8,
        ]);
    }
    flattened
}

#[cfg(test)]
#[path = "theme_background_image_tests.rs"]
mod tests;
