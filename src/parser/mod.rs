//! NAL parsing: Annex B scan, H.264/H.265 NAL list and detail.

mod annex_b;
mod h264;
mod h265;

pub use annex_b::scan_nal_units;
pub use h264::parse_h264_file;
pub use h264::parse_nal_detail;
pub use h264::tree_text_for_nal;
pub use h265::parse_h265_file;

use std::fs;
use std::path::Path;

use crate::model::{guess_file_type, ParseResult};

/// Parse H.264 or H.265 file based on path and content.
pub fn parse_file(path: &Path) -> std::io::Result<ParseResult> {
    let data = fs::read(path)?;
    let prefix = if data.len() >= 8 { Some(data.as_slice()) } else { None };
    let ft = guess_file_type(path, prefix);
    match ft {
        crate::model::FileType::H265 => h265::parse_h265_bytes(&data),
        _ => h264::parse_h264_bytes(&data, path),
    }
}
