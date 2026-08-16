//! Safe image-underlay decoding. Geometry and UI texture ownership stay in the app.
use image::ImageReader;
use std::io::Cursor;

#[derive(Clone, Copy, Debug)]
pub struct UnderlayLimits {
    pub max_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
}
impl Default for UnderlayLimits {
    fn default() -> Self {
        Self {
            max_bytes: 32 << 20,
            max_width: 16_384,
            max_height: 16_384,
            max_pixels: 40_000_000,
        }
    }
}
#[derive(Clone, Debug)]
pub struct DecodedUnderlay {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub source_hash: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnderlayError {
    TooLarge,
    Unsupported,
    Invalid,
    Dimensions,
    Allocation,
}
impl std::fmt::Display for UnderlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "image exceeds the 32 MiB safety limit",
            Self::Unsupported => "only PNG and JPEG underlays are supported",
            Self::Invalid => "image is malformed or truncated",
            Self::Dimensions => "image dimensions exceed the safe display limit",
            Self::Allocation => "decoded image size is unsafe",
        })
    }
}
impl std::error::Error for UnderlayError {}

pub fn decode_underlay(
    bytes: &[u8],
    limits: UnderlayLimits,
) -> Result<DecodedUnderlay, UnderlayError> {
    if bytes.len() > limits.max_bytes {
        return Err(UnderlayError::TooLarge);
    }
    let format = image::guess_format(bytes).map_err(|_| UnderlayError::Unsupported)?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err(UnderlayError::Unsupported);
    }
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| UnderlayError::Invalid)?;
    let pixels = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or(UnderlayError::Allocation)?;
    if w == 0
        || h == 0
        || w > limits.max_width
        || h > limits.max_height
        || pixels > limits.max_pixels
    {
        return Err(UnderlayError::Dimensions);
    }
    let required = pixels
        .checked_mul(4)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(UnderlayError::Allocation)?;
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| UnderlayError::Invalid)?
        .into_rgba8();
    if decoded.len() != required {
        return Err(UnderlayError::Allocation);
    }
    Ok(DecodedUnderlay {
        width: w,
        height: h,
        rgba: decoded.into_raw(),
        source_hash: *blake3::hash(bytes).as_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_decodes_and_hashes() {
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let x = decode_underlay(&png, Default::default()).unwrap();
        assert_eq!((x.width, x.height, x.rgba.len()), (1, 1, 4));
    }
    #[test]
    fn hostile_sizes_and_formats_rejected() {
        let l = UnderlayLimits {
            max_bytes: 2,
            ..Default::default()
        };
        assert_eq!(
            decode_underlay(b"123", l).unwrap_err(),
            UnderlayError::TooLarge
        );
        assert_eq!(
            decode_underlay(b"not image", Default::default()).unwrap_err(),
            UnderlayError::Unsupported
        );
    }
}
