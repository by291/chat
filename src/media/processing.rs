//! Actual byte inspection and bounded, sanitized image derivation.

use std::{io::Cursor, sync::Arc};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::error::AppError;

const MAX_DIMENSION: u32 = 8192;
const MAX_PIXELS: u64 = 20_000_000;
const MAX_DECODE_ALLOCATION: u64 = 128 * 1024 * 1024;
const THUMBNAIL_DIMENSION: u32 = 512;

pub struct InspectedMedia {
    pub content_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: String,
    pub thumbnail: Option<Vec<u8>>,
    pub avatar: Option<Vec<u8>>,
}

/// Decode and generate previews with the permit owned by the blocking task through exit.
pub async fn process(
    bytes: Vec<u8>,
    claimed_type: String,
    avatar: bool,
    processors: Arc<Semaphore>,
) -> Result<InspectedMedia, AppError> {
    let permit = processors
        .acquire_owned()
        .await
        .map_err(|_| AppError::unavailable())?;

    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        inspect(&bytes, &claimed_type, avatar)
    })
    .await
    .map_err(|_| AppError::unavailable())?
}

/// Verify format/dimensions before allocating decoded pixels; generic files stay downloads.
pub fn inspect(bytes: &[u8], claimed_type: &str, avatar: bool) -> Result<InspectedMedia, AppError> {
    let detected = image::guess_format(bytes).ok();
    let supported = matches!(
        detected,
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    );
    let claimed_image = matches!(claimed_type, "image/jpeg" | "image/png" | "image/webp");
    if !supported && (avatar || claimed_image) {
        return Err(AppError::invalid("媒体图片格式无效"));
    }

    let mut inspected = InspectedMedia {
        content_type: "application/octet-stream".into(),
        width: None,
        height: None,
        sha256: hex::encode(Sha256::digest(bytes)),
        thumbnail: None,
        avatar: None,
    };
    if !supported {
        return Ok(inspected);
    }

    let format = detected.ok_or_else(|| AppError::invalid("媒体图片格式无效"))?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    // image documents max_alloc as best effort; strict dimensions and two permits also bound expansion.
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| AppError::invalid("媒体图片无法解码"))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(AppError::invalid("媒体图片尺寸超过限制"));
    }

    let image =
        DynamicImage::from_decoder(decoder).map_err(|_| AppError::invalid("媒体图片无法解码"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_DIMENSION, THUMBNAIL_DIMENSION);
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| AppError::invalid("媒体图片无法处理"))?;
    let sanitized = encoded.into_inner();

    inspected.content_type = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::WebP => "image/webp",
        _ => return Err(AppError::invalid("媒体图片格式无效")),
    }
    .into();
    inspected.width = Some(width);
    inspected.height = Some(height);
    inspected.thumbnail = Some(sanitized.clone());
    if avatar {
        inspected.avatar = Some(sanitized);
    }

    Ok(inspected)
}
