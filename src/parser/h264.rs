//! H.264 file parsing: NAL list, SPS/PPS info, and detail text for tree view.

use h264_parser::parser::AnnexBParser;
use h264_parser::sps::Sps;
use h264_parser::pps::Pps;
use std::fs;
use std::path::Path;

use crate::model::{
    FileType, NaluInfo, NalUnitType, ParseResult, SliceType, SpsInfo, PpsInfo,
};
use crate::parser::annex_b::scan_nal_units;

/// Parse H.264 Annex B file: NAL list + optional SPS/PPS info.
pub fn parse_h264_file(path: &Path) -> std::io::Result<ParseResult> {
    let data = fs::read(path)?;
    parse_h264_bytes(&data, path)
}

/// Parse H.264 from memory (path used for file type hint).
pub fn parse_h264_bytes(data: &[u8], _path: &Path) -> std::io::Result<ParseResult> {
    let spans = scan_nal_units(data);
    let mut nalus = Vec::with_capacity(spans.len());
    for (i, span) in spans.iter().enumerate() {
        let data_start = span.data_start as usize;
        let data_end = span.data_end as usize;
        if data_end > data.len() {
            continue;
        }
        let nal_type_byte = data.get(data_start).copied().unwrap_or(0);
        let nal_type = NalUnitType::from(nal_type_byte & 0x1F);
        let slice_type = slice_type_from_nal(data, data_start, data_end);
        let raw = data[span.start_pos as usize..data_end].to_vec();
        nalus.push(NaluInfo {
            index: i as u32,
            offset: span.start_pos,
            len: span.len(),
            start_code_len: span.start_code_len,
            nal_type,
            h265_nal_type: None,
            slice_type,
            raw,
        });
    }

    let (sps_info, pps_info) = extract_sps_pps(data);

    Ok(ParseResult {
        file_type: FileType::H264,
        nalus,
        sps_info,
        pps_info,
    })
}

/// Try to read slice_type from slice NAL (first ue() in slice header is slice_type).
fn slice_type_from_nal(data: &[u8], data_start: usize, data_end: usize) -> SliceType {
    if data_end <= data_start + 1 {
        return SliceType::Unknown;
    }
    let ebsp = &data[data_start + 1..data_end];
    let rbsp = h264_parser::nal::ebsp_to_rbsp(ebsp);
    let slice_type = read_ue(&rbsp, 0).map(|(v, _)| v).unwrap_or(9);
    match slice_type {
        0 => SliceType::P,
        1 => SliceType::B,
        2 => SliceType::I,
        _ => SliceType::Unknown,
    }
}

/// Read one unsigned Exp-Golomb from bit offset; returns (value, new_bit_offset).
fn read_ue(data: &[u8], start_bits: usize) -> Option<(u32, usize)> {
    let mut bit = start_bits;
    let mut zeros = 0u32;
    while bit / 8 < data.len() {
        let b = data[bit / 8];
        let shift = 7 - (bit % 8);
        if (b >> shift) & 1 != 0 {
            break;
        }
        zeros += 1;
        bit += 1;
    }
    bit += 1;
    if zeros > 31 {
        return None;
    }
    let mut val = 0u32;
    for _ in 0..zeros {
        if bit / 8 >= data.len() {
            return None;
        }
        val = (val << 1) | (((data[bit / 8] >> (7 - (bit % 8))) & 1) as u32);
        bit += 1;
    }
    Some(( (1 << zeros) - 1 + val, bit ))
}

/// Run Annex B parser to extract first SPS and PPS.
fn extract_sps_pps(data: &[u8]) -> (Option<SpsInfo>, Option<PpsInfo>) {
    let mut parser = AnnexBParser::default();
    parser.push(data);
    let mut sps_info = None;
    let mut pps_info = None;
    while let Ok(Some(au)) = parser.next_access_unit() {
        if sps_info.is_none() {
            if let Some(ref sps) = au.sps {
                sps_info = Some(sps_to_info(sps));
            }
        }
        if pps_info.is_none() {
            if let Some(ref pps) = au.pps {
                pps_info = Some(pps_to_info(pps));
            }
        }
        if sps_info.is_some() && pps_info.is_some() {
            break;
        }
    }
    (sps_info, pps_info)
}

fn sps_to_info(s: &Sps) -> SpsInfo {
    let (crop_l, crop_r, crop_t, crop_b) = if s.frame_cropping_flag {
        (
            s.frame_crop_left_offset * 2,
            s.frame_crop_right_offset * 2,
            s.frame_crop_top_offset * 2,
            s.frame_crop_bottom_offset * 2,
        )
    } else {
        (0, 0, 0, 0)
    };
    SpsInfo {
        profile_idc: s.profile_idc,
        level_idc: s.level_idc,
        width: s.width.saturating_sub(crop_l).saturating_sub(crop_r),
        height: s.height.saturating_sub(crop_t).saturating_sub(crop_b),
        crop_left: crop_l,
        crop_right: crop_r,
        crop_top: crop_t,
        crop_bottom: crop_b,
        max_framerate: 0.0,
        chroma_format_idc: s.chroma_format_idc,
    }
}

fn pps_to_info(p: &Pps) -> PpsInfo {
    PpsInfo {
        entropy_coding_mode_flag: p.entropy_coding_mode_flag,
    }
}

/// Build tree/detail text for a NAL (for hex view and detail panel).
pub fn parse_nal_detail(nal_raw: &[u8]) -> String {
    if nal_raw.len() < 4 {
        return format!("Invalid NAL (len={})", nal_raw.len());
    }
    let (start_code_len, header_byte) = if nal_raw.starts_with(&[0, 0, 0, 1]) {
        (4, nal_raw.get(4).copied().unwrap_or(0))
    } else if nal_raw.starts_with(&[0, 0, 1]) {
        (3, nal_raw.get(3).copied().unwrap_or(0))
    } else {
        return "No Annex B start code".to_string();
    };
    let ref_idc = (header_byte >> 5) & 3;
    let nal_type = header_byte & 0x1F;
    let mut lines = vec![
        "NAL Unit".to_string(),
        format!("  Start code length: {} bytes", start_code_len),
        format!("  nal_ref_idc: {}", ref_idc),
        format!("  nal_unit_type: {} ({})", nal_type, NalUnitType::from(nal_type)),
        format!("  Length: {} bytes", nal_raw.len()),
    ];
    if start_code_len + 1 <= nal_raw.len() {
        let ebsp = &nal_raw[start_code_len + 1..];
        let rbsp = h264_parser::nal::ebsp_to_rbsp(ebsp);
        match nal_type {
            7 => {
                if let Some(sps) = parse_sps_rbsp(&rbsp) {
                    lines.push("  SPS:".to_string());
                    lines.push(format!("    profile_idc: {}", sps.profile_idc));
                    lines.push(format!("    level_idc: {}", sps.level_idc));
                    lines.push(format!("    width: {} height: {}", sps.width, sps.height));
                    lines.push(format!("    chroma_format_idc: {}", sps.chroma_format_idc));
                }
            }
            8 => {
                if let Some(pps) = parse_pps_rbsp(&rbsp) {
                    lines.push("  PPS:".to_string());
                    lines.push(format!("    entropy_coding_mode_flag: {}", pps.entropy_coding_mode_flag));
                }
            }
            1 | 5 => {
                if let Some((st, _)) = read_ue(&rbsp, 0) {
                    let st_str = match st {
                        0 => "P",
                        1 => "B",
                        2 => "I",
                        3 => "Sp",
                        4 => "Si",
                        _ => "?",
                    };
                    lines.push(format!("  slice_type: {} ({})", st, st_str));
                }
            }
            _ => {}
        }
    }
    lines.join("\n")
}

/// Parse SPS RBSP (after NAL header) - minimal parse for display.
fn parse_sps_rbsp(rbsp: &[u8]) -> Option<Sps> {
    Sps::parse(rbsp).ok()
}

fn parse_pps_rbsp(rbsp: &[u8]) -> Option<Pps> {
    Pps::parse(rbsp).ok()
}

/// Tree-style text for the selected NAL (same as detail, can be extended with tree formatting).
pub fn tree_text_for_nal(nal_raw: &[u8]) -> String {
    parse_nal_detail(nal_raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let data = [];
        let r = parse_h264_bytes(&data, Path::new("x.h264")).unwrap();
        assert_eq!(r.nalus.len(), 0);
    }
}
