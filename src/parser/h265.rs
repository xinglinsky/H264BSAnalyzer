//! H.265/HEVC file parsing: NAL list (scan + type); no SPS/PPS detail yet.

use std::fs;
use std::path::Path;

use crate::model::{FileType, NaluInfo, NalUnitType, ParseResult, SliceType};
use crate::parser::annex_b::scan_nal_units;

/// Parse H.265 Annex B file: NAL list with type from header.
pub fn parse_h265_file(path: &Path) -> std::io::Result<ParseResult> {
    let data = fs::read(path)?;
    parse_h265_bytes(&data)
}

/// Parse H.265 from memory.
pub fn parse_h265_bytes(data: &[u8]) -> std::io::Result<ParseResult> {
    let spans = scan_nal_units(data);
    let mut nalus = Vec::with_capacity(spans.len());
    for (i, span) in spans.iter().enumerate() {
        let data_start = span.data_start as usize;
        let data_end = span.data_end as usize;
        if data_end > data.len() || data_start + 2 > data.len() {
            continue;
        }
        let b0 = data[data_start];
        let h265_type = (b0 >> 1) & 0x3F;
        let nal_type = NalUnitType::Unspecified;
        let raw = data[span.start_pos as usize..data_end].to_vec();
        nalus.push(NaluInfo {
            index: i as u32,
            offset: span.start_pos,
            len: span.len(),
            start_code_len: span.start_code_len,
            nal_type,
            h265_nal_type: Some(h265_type),
            slice_type: SliceType::Unknown,
            raw,
        });
    }
    Ok(ParseResult {
        file_type: FileType::H265,
        nalus,
        sps_info: None,
        pps_info: None,
    })
}
