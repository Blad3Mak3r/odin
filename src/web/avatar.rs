//! Profile pictures: whatever image an account uploads is cropped to a square,
//! shrunk and re-encoded as WebP, so the database only ever holds a small,
//! known-good file regardless of what was sent.

use std::io::Cursor;

use anyhow::Result;
use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, ImageReader, Limits};

use crate::web::error::BadRequest;

/// Stored edge length. The dashboard shows avatars at 80px or less, so this
/// stays sharp on 2x screens.
pub const AVATAR_SIZE: u32 = 192;
/// Largest accepted upload; the dashboard sends a 512px crop, far below this.
pub const MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;
/// Decoder limits against decompression bombs (a tiny file can claim a
/// gigantic canvas).
const MAX_DIMENSION: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;

fn unsupported() -> anyhow::Error {
    BadRequest("the file is not a supported image (use JPEG, PNG, GIF or WebP)".into()).into()
}

/// Decodes `bytes` (JPEG, PNG, GIF or WebP), center-crops to a square, resizes
/// to [`AVATAR_SIZE`] and returns it as lossless WebP.
pub fn encode_avatar(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| unsupported())?;
    reader.limits(limits);
    let image = reader.decode().map_err(|_| unsupported())?;

    let square = image
        .resize_to_fill(AVATAR_SIZE, AVATAR_SIZE, FilterType::Lanczos3)
        .into_rgba8();
    let mut webp = Vec::new();
    WebPEncoder::new_lossless(&mut webp).write_image(
        square.as_raw(),
        AVATAR_SIZE,
        AVATAR_SIZE,
        ExtendedColorType::Rgba8,
    )?;
    Ok(webp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        });
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn crops_and_shrinks_to_a_square_webp() {
        let webp = encode_avatar(&png(800, 400)).unwrap();
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let decoded = image::load_from_memory_with_format(&webp, ImageFormat::WebP).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (AVATAR_SIZE, AVATAR_SIZE)
        );
    }

    #[test]
    fn small_images_are_scaled_up_to_the_stored_size() {
        let webp = encode_avatar(&png(32, 32)).unwrap();
        let decoded = image::load_from_memory_with_format(&webp, ImageFormat::WebP).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (AVATAR_SIZE, AVATAR_SIZE)
        );
    }

    #[test]
    fn rejects_bytes_that_are_not_an_image() {
        let err = encode_avatar(b"definitely not an image").unwrap_err();
        assert!(err.downcast_ref::<BadRequest>().is_some());
    }

    #[test]
    fn rejects_images_over_the_dimension_limit() {
        let err = encode_avatar(&png(MAX_DIMENSION + 1, 1)).unwrap_err();
        assert!(err.downcast_ref::<BadRequest>().is_some());
    }
}
