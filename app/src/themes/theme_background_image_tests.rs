use image::{ImageFormat, Rgba, RgbaImage};

use super::*;

/// Writes a PNG with an alpha channel to a temp dir and returns (dir, file).
fn rgba_png(width: u32, height: u32, pixel: Rgba<u8>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("source.png");
    let mut image = RgbaImage::new(width, height);
    for p in image.pixels_mut() {
        *p = pixel;
    }
    image
        .save_with_format(&path, ImageFormat::Png)
        .expect("write source");
    (dir, path)
}

#[test]
fn oversized_images_are_capped_on_the_longest_edge() {
    let (dir, source) = rgba_png(3456, 2234, Rgba([120, 40, 200, 255]));

    let written = import(&source, dir.path(), "capped").expect("import");

    let out = image::open(&written).expect("read output");
    assert_eq!(out.width().max(out.height()), MAX_EDGE);
    // Aspect ratio preserved to within a pixel of rounding.
    let expected_h = (2234.0 * (MAX_EDGE as f32 / 3456.0)).ceil() as u32;
    assert!(
        out.height().abs_diff(expected_h) <= 1,
        "expected about {expected_h} tall, got {}",
        out.height(),
    );
}

#[test]
fn images_within_the_cap_keep_their_dimensions() {
    let (dir, source) = rgba_png(1600, 900, Rgba([10, 20, 30, 255]));

    let written = import(&source, dir.path(), "untouched").expect("import");

    let out = image::open(&written).expect("read output");
    assert_eq!((out.width(), out.height()), (1600, 900));
}

#[test]
fn output_is_always_jpeg_without_an_alpha_channel() {
    let (dir, source) = rgba_png(64, 64, Rgba([200, 100, 50, 255]));

    let written = import(&source, dir.path(), "flattened").expect("import");

    assert_eq!(written.extension().and_then(|e| e.to_str()), Some("jpg"));
    assert_eq!(
        image::ImageReader::open(&written)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .format(),
        Some(ImageFormat::Jpeg),
    );
    assert!(!image::open(&written).unwrap().color().has_alpha());
}

/// A fully transparent source must come out black, not with whatever colour was hiding under the
/// alpha. This is the case `to_rgb8` alone gets wrong.
#[test]
fn transparent_pixels_composite_onto_black() {
    let (dir, source) = rgba_png(32, 32, Rgba([255, 0, 0, 0]));

    let written = import(&source, dir.path(), "transparent").expect("import");

    let out = image::open(&written).expect("read output").to_rgb8();
    let sampled = out.get_pixel(16, 16);
    assert!(
        sampled[0] < 12 && sampled[1] < 12 && sampled[2] < 12,
        "fully transparent red should composite to black, got {sampled:?}",
    );
}

/// Opaque pixels must survive the alpha path untouched, so importing an RGBA-but-opaque image
/// (which is what a macOS screenshot is) does not shift its colours.
#[test]
fn opaque_pixels_are_unchanged_by_flattening() {
    let (dir, source) = rgba_png(32, 32, Rgba([37, 150, 190, 255]));

    let written = import(&source, dir.path(), "opaque").expect("import");

    let out = image::open(&written).expect("read output").to_rgb8();
    let sampled = out.get_pixel(16, 16);
    // JPEG is lossy, so allow a small tolerance rather than demanding exact equality.
    assert!(
        sampled[0].abs_diff(37) <= 6
            && sampled[1].abs_diff(150) <= 6
            && sampled[2].abs_diff(190) <= 6,
        "expected roughly (37, 150, 190), got {sampled:?}",
    );
}

#[test]
fn a_missing_source_is_an_error_rather_than_a_panic() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nope.png");

    assert!(import(&missing, dir.path(), "missing").is_err());
}

/// Importing writes a preview thumbnail beside the full image, much smaller on the longest edge.
/// The explorer and picker draw cards from the thumbnail so they never decode the full wallpaper.
#[test]
fn import_writes_a_smaller_thumbnail_beside_the_full_image() {
    let (dir, source) = rgba_png(2000, 1200, Rgba([90, 140, 210, 255]));

    let full = import(&source, dir.path(), "with-thumb").expect("import");
    let thumb = thumbnail_path(&full);

    assert!(
        thumb.exists(),
        "a thumbnail should sit beside the full image"
    );

    let full_edge = {
        let img = image::open(&full).unwrap();
        img.width().max(img.height())
    };
    let thumb_img = image::open(&thumb).unwrap();
    assert_eq!(thumb_img.width().max(thumb_img.height()), THUMB_EDGE);
    assert!(
        thumb_img.width().max(thumb_img.height()) < full_edge,
        "thumbnail should be smaller than the full image",
    );
    // The thumbnail is a real preview, not the placeholder the fallback would show.
    assert!(!thumb_img.color().has_alpha());
}

/// `thumbnail_path` is a pure sibling derivation — no filesystem — so it is cheap to call per card.
#[test]
fn thumbnail_path_is_a_sibling_of_the_full_image() {
    let derived = thumbnail_path(std::path::Path::new("/themes/community/koi.jpg"));
    assert_eq!(
        derived,
        std::path::PathBuf::from("/themes/community/koi.thumb.jpg"),
    );
}

/// Downloaded images arrive as bytes rather than a file, and must be normalised the same way.
#[test]
fn importing_from_bytes_caps_and_thumbnails_too() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut buffer = std::io::Cursor::new(Vec::new());
    RgbaImage::from_pixel(3000, 2000, Rgba([200, 50, 50, 255]))
        .write_to(&mut buffer, ImageFormat::Png)
        .expect("encode source");

    let full = import_bytes(buffer.get_ref(), dir.path(), "downloaded").expect("import bytes");

    let full_img = image::open(&full).unwrap();
    assert_eq!(full_img.width().max(full_img.height()), MAX_EDGE);
    assert!(thumbnail_path(&full).exists());
}
