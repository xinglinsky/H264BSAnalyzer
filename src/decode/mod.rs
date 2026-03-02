//! Video decode and frame output (optional FFmpeg/rsmpeg).

use std::path::Path;

/// Decoded frame: RGB24, width, height.
#[derive(Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Decoder state for playback.
#[cfg(not(feature = "decode"))]
pub struct Decoder;

#[cfg(not(feature = "decode"))]
impl Decoder {
    pub fn open(_path: &Path) -> Result<Self, String> {
        Err("Playback requires FFmpeg. Build with: cargo build --features decode. Install FFmpeg development libraries.".to_string())
    }

    pub fn width(&self) -> u32 {
        0
    }

    pub fn height(&self) -> u32 {
        0
    }

    pub fn next_frame(&mut self) -> Result<Option<DecodedFrame>, String> {
        Ok(None)
    }

    pub fn flush_frames(&mut self) -> Result<Option<DecodedFrame>, String> {
        Ok(None)
    }
}

#[cfg(feature = "decode")]
mod ffmpeg;

#[cfg(feature = "decode")]
pub use ffmpeg::Decoder;
