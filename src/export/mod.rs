//! Export decoded frames to YUV, RGB, BMP, JPEG.

use std::path::Path;

use image::{ImageBuffer, RgbImage};

use crate::decode::DecodedFrame;

/// Export RGB frame to BMP file.
pub fn export_bmp(path: &Path, frame: &DecodedFrame) -> Result<(), String> {
    let w = frame.width as u32;
    let h = frame.height as u32;
    if frame.rgb.len() < (w * h * 3) as usize {
        return Err("frame too small".to_string());
    }
    let img: RgbImage = ImageBuffer::from_raw(w, h, frame.rgb.clone())
        .ok_or_else(|| "invalid dimensions".to_string())?;
    img.save(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Export RGB frame to JPEG file.
pub fn export_jpeg(path: &Path, frame: &DecodedFrame) -> Result<(), String> {
    let w = frame.width as u32;
    let h = frame.height as u32;
    if frame.rgb.len() < (w * h * 3) as usize {
        return Err("frame too small".to_string());
    }
    let img: RgbImage = ImageBuffer::from_raw(w, h, frame.rgb.clone())
        .ok_or_else(|| "invalid dimensions".to_string())?;
    img.save(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Export RGB frame to raw RGB24 file (no header).
pub fn export_rgb(path: &Path, frame: &DecodedFrame) -> Result<(), String> {
    std::fs::write(path, &frame.rgb).map_err(|e| e.to_string())?;
    Ok(())
}

/// Convert RGB24 to YUV420P (BT.601).
fn rgb_to_yuv420p(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut yuv = Vec::with_capacity(w * h * 3 / 2);
    for chunk in rgb.chunks_exact(3) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        let y = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
        yuv.push(y);
    }
    let uv_w = w / 2;
    let uv_h = h / 2;
    for j in 0..uv_h {
        for i in 0..uv_w {
            let px = (j * 2) * w + (i * 2);
            let r = (rgb[px * 3] as f32 + rgb[px * 3 + 3] as f32
                + rgb[(px + w) * 3] as f32
                + rgb[(px + w) * 3 + 3] as f32)
                / 4.0;
            let g = (rgb[px * 3 + 1] as f32 + rgb[px * 3 + 4] as f32
                + rgb[(px + w) * 3 + 1] as f32
                + rgb[(px + w) * 3 + 4] as f32)
                / 4.0;
            let b = (rgb[px * 3 + 2] as f32 + rgb[px * 3 + 5] as f32
                + rgb[(px + w) * 3 + 2] as f32
                + rgb[(px + w) * 3 + 5] as f32)
                / 4.0;
            let cb = (128.0 - 0.169 * r - 0.331 * g + 0.5 * b).round().clamp(0.0, 255.0) as u8;
            let cr = (128.0 + 0.5 * r - 0.419 * g - 0.081 * b).round().clamp(0.0, 255.0) as u8;
            yuv.push(cb);
            yuv.push(cr);
        }
    }
    yuv
}

/// Export frame as YUV420P raw file.
pub fn export_yuv(path: &Path, frame: &DecodedFrame) -> Result<(), String> {
    let yuv = rgb_to_yuv420p(&frame.rgb, frame.width, frame.height);
    std::fs::write(path, &yuv).map_err(|e| e.to_string())?;
    Ok(())
}
