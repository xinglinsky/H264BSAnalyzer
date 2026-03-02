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

/// Try to read slice_type from slice NAL (second ue() in slice header).
fn slice_type_from_nal(data: &[u8], data_start: usize, data_end: usize) -> SliceType {
    if data_end <= data_start + 1 {
        return SliceType::Unknown;
    }
    let ebsp = &data[data_start + 1..data_end];
    let rbsp = h264_parser::nal::ebsp_to_rbsp(ebsp);
    let slice_type = read_ue(&rbsp, 0)
        .and_then(|(_, bit)| read_ue(&rbsp, bit).map(|(v, _)| v))
        .unwrap_or(9);
    match slice_type % 5 {
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

struct H264BitReader<'a> {
    data: &'a [u8],
    bit_offset: usize,
}

impl<'a> H264BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_offset: 0 }
    }
    fn read_bit(&mut self) -> Option<u8> {
        if self.bit_offset / 8 >= self.data.len() {
            return None;
        }
        let b = self.data[self.bit_offset / 8];
        let shift = 7 - (self.bit_offset % 8);
        self.bit_offset += 1;
        Some((b >> shift) & 1)
    }
    fn read_bits(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | (self.read_bit()? as u32);
        }
        Some(v)
    }
    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0u32;
        while self.read_bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let mut val = 0u32;
        for _ in 0..zeros {
            val = (val << 1) | (self.read_bit()? as u32);
        }
        Some((1 << zeros) - 1 + val)
    }
    fn read_se(&mut self) -> Option<i32> {
        let v = self.read_ue()? as i32;
        if v & 1 == 0 {
            Some(-(v / 2))
        } else {
            Some((v + 1) / 2)
        }
    }
}

fn skip_scaling_list_h264(br: &mut H264BitReader<'_>, size: usize) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = br.read_se()?;
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        last_scale = if next_scale == 0 { last_scale } else { next_scale };
    }
    Some(())
}

fn parse_h264_sps_max_fps(rbsp: &[u8]) -> Option<f32> {
    let mut br = H264BitReader::new(rbsp);
    let profile_idc = br.read_bits(8)? as u8;
    let _constraint_and_reserved = br.read_bits(8)?;
    let _level_idc = br.read_bits(8)?;
    let _seq_parameter_set_id = br.read_ue()?;

    if matches!(profile_idc, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135) {
        let chroma_format_idc = br.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane_flag = br.read_bit()?;
        }
        let _bit_depth_luma_minus8 = br.read_ue()?;
        let _bit_depth_chroma_minus8 = br.read_ue()?;
        let _qpprime_y_zero_transform_bypass_flag = br.read_bit()?;
        let seq_scaling_matrix_present_flag = br.read_bit()?;
        if seq_scaling_matrix_present_flag != 0 {
            let num_lists = if chroma_format_idc != 3 { 8usize } else { 12usize };
            for i in 0..num_lists {
                let seq_scaling_list_present_flag = br.read_bit()?;
                if seq_scaling_list_present_flag != 0 {
                    let size = if i < 6 { 16 } else { 64 };
                    skip_scaling_list_h264(&mut br, size)?;
                }
            }
        }
    }

    let _log2_max_frame_num_minus4 = br.read_ue()?;
    let pic_order_cnt_type = br.read_ue()?;
    if pic_order_cnt_type == 0 {
        let _log2_max_pic_order_cnt_lsb_minus4 = br.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        let _delta_pic_order_always_zero_flag = br.read_bit()?;
        let _offset_for_non_ref_pic = br.read_se()?;
        let _offset_for_top_to_bottom_field = br.read_se()?;
        let num_ref_frames_in_pic_order_cnt_cycle = br.read_ue()?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            let _offset_for_ref_frame = br.read_se()?;
        }
    }

    let _max_num_ref_frames = br.read_ue()?;
    let _gaps_in_frame_num_value_allowed_flag = br.read_bit()?;
    let _pic_width_in_mbs_minus1 = br.read_ue()?;
    let _pic_height_in_map_units_minus1 = br.read_ue()?;
    let frame_mbs_only_flag = br.read_bit()?;
    if frame_mbs_only_flag == 0 {
        let _mb_adaptive_frame_field_flag = br.read_bit()?;
    }
    let _direct_8x8_inference_flag = br.read_bit()?;
    let frame_cropping_flag = br.read_bit()?;
    if frame_cropping_flag != 0 {
        let _frame_crop_left_offset = br.read_ue()?;
        let _frame_crop_right_offset = br.read_ue()?;
        let _frame_crop_top_offset = br.read_ue()?;
        let _frame_crop_bottom_offset = br.read_ue()?;
    }

    let vui_parameters_present_flag = br.read_bit()?;
    if vui_parameters_present_flag == 0 {
        return None;
    }

    let aspect_ratio_info_present_flag = br.read_bit()?;
    if aspect_ratio_info_present_flag != 0 {
        let aspect_ratio_idc = br.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            let _sar_width = br.read_bits(16)?;
            let _sar_height = br.read_bits(16)?;
        }
    }
    let overscan_info_present_flag = br.read_bit()?;
    if overscan_info_present_flag != 0 {
        let _overscan_appropriate_flag = br.read_bit()?;
    }
    let video_signal_type_present_flag = br.read_bit()?;
    if video_signal_type_present_flag != 0 {
        let _video_format = br.read_bits(3)?;
        let _video_full_range_flag = br.read_bit()?;
        let colour_description_present_flag = br.read_bit()?;
        if colour_description_present_flag != 0 {
            let _colour_primaries = br.read_bits(8)?;
            let _transfer_characteristics = br.read_bits(8)?;
            let _matrix_coefficients = br.read_bits(8)?;
        }
    }
    let chroma_loc_info_present_flag = br.read_bit()?;
    if chroma_loc_info_present_flag != 0 {
        let _chroma_sample_loc_type_top_field = br.read_ue()?;
        let _chroma_sample_loc_type_bottom_field = br.read_ue()?;
    }

    let timing_info_present_flag = br.read_bit()?;
    if timing_info_present_flag == 0 {
        return None;
    }
    let num_units_in_tick = br.read_bits(32)?;
    let time_scale = br.read_bits(32)?;
    let _fixed_frame_rate_flag = br.read_bit()?;
    if num_units_in_tick == 0 {
        return None;
    }
    Some((time_scale as f32) / (2.0 * num_units_in_tick as f32))
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

    if let Some(ref mut s) = sps_info {
        let mut best_fps = 0.0f32;
        for span in scan_nal_units(data) {
            let data_start = span.data_start as usize;
            let data_end = span.data_end as usize;
            if data_end > data.len() || data_start + 1 > data_end {
                continue;
            }
            let nal_type = data[data_start] & 0x1F;
            if nal_type != 7 {
                continue;
            }
            let ebsp = &data[data_start + 1..data_end];
            let rbsp = h264_parser::nal::ebsp_to_rbsp(ebsp);
            if let Some(fps) = parse_h264_sps_max_fps(&rbsp) {
                if fps > best_fps {
                    best_fps = fps;
                }
            }
        }
        if best_fps > 0.0 {
            s.max_framerate = best_fps;
        }
    }

    (sps_info, pps_info)
}

fn sps_to_info(s: &Sps) -> SpsInfo {
    let (crop_l, crop_r, crop_t, crop_b) = if s.frame_cropping_flag {
        (
            s.frame_crop_left_offset,
            s.frame_crop_right_offset,
            s.frame_crop_top_offset,
            s.frame_crop_bottom_offset,
        )
    } else {
        (0, 0, 0, 0)
    };
    SpsInfo {
        profile_idc: s.profile_idc,
        level_idc: s.level_idc,
        width: s.width,
        height: s.height,
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
    let forbidden_zero_bit = (header_byte >> 7) & 1;
    let ref_idc = (header_byte >> 5) & 3;
    let nal_type = header_byte & 0x1F;
    let mut lines = vec![
        "NAL".to_string(),
        "  nal_unit_header".to_string(),
        format!("    forbidden_zero_bit: {} (1 bit)", forbidden_zero_bit),
        format!("    nal_ref_idc: {} (2 bits)", ref_idc),
        format!(
            "    nal_unit_type: {} ({})(5 bits)",
            nal_type,
            NalUnitType::from(nal_type)
        ),
        format!("  Length: {} bytes", nal_raw.len()),
    ];
    if start_code_len + 1 <= nal_raw.len() {
        let ebsp = &nal_raw[start_code_len + 1..];
        let rbsp = h264_parser::nal::ebsp_to_rbsp(ebsp);
        match nal_type {
            7 => {
                if let Some(sps) = parse_sps_rbsp(&rbsp) {
                    lines.push("  sequence_parameter_set_rbsp():".to_string());
                    lines.push(format!("    profile_idc: {} (8 bits)", sps.profile_idc));
                    lines.push(format!("    constraint_set0_flag: {} [{}] (1 bit)", sps.constraint_set0_flag as u8, sps.constraint_set0_flag));
                    lines.push(format!("    constraint_set1_flag: {} [{}] (1 bit)", sps.constraint_set1_flag as u8, sps.constraint_set1_flag));
                    lines.push(format!("    constraint_set2_flag: {} [{}] (1 bit)", sps.constraint_set2_flag as u8, sps.constraint_set2_flag));
                    lines.push(format!("    constraint_set3_flag: {} [{}] (1 bit)", sps.constraint_set3_flag as u8, sps.constraint_set3_flag));
                    lines.push(format!("    constraint_set4_flag: {} [{}] (1 bit)", sps.constraint_set4_flag as u8, sps.constraint_set4_flag));
                    lines.push(format!("    constraint_set5_flag: {} [{}] (1 bit)", sps.constraint_set5_flag as u8, sps.constraint_set5_flag));
                    lines.push(format!("    level_idc: {} (8 bits)", sps.level_idc));
                    lines.push(format!("    seq_parameter_set_id: {} (ue)", sps.seq_parameter_set_id));
                    lines.push(format!("    chroma_format_idc: {} (ue)", sps.chroma_format_idc));
                    lines.push(format!("    separate_colour_plane_flag: {} [{}] (1 bit)", sps.separate_colour_plane_flag as u8, sps.separate_colour_plane_flag));
                    lines.push(format!("    bit_depth_luma_minus8: {} (ue)", sps.bit_depth_luma_minus8));
                    lines.push(format!("    bit_depth_chroma_minus8: {} (ue)", sps.bit_depth_chroma_minus8));
                    lines.push(format!("    qpprime_y_zero_transform_bypass_flag: {} [{}] (1 bit)", sps.qpprime_y_zero_transform_bypass_flag as u8, sps.qpprime_y_zero_transform_bypass_flag));
                    lines.push(format!("    seq_scaling_matrix_present_flag: {} [{}] (1 bit)", sps.seq_scaling_matrix_present_flag as u8, sps.seq_scaling_matrix_present_flag));
                    lines.push(format!("    log2_max_frame_num_minus4: {} (ue)", sps.log2_max_frame_num_minus4));
                    lines.push(format!("    pic_order_cnt_type: {} (ue)", sps.pic_order_cnt_type));
                    lines.push(format!("    log2_max_pic_order_cnt_lsb_minus4: {} (ue)", sps.log2_max_pic_order_cnt_lsb_minus4));
                    lines.push(format!("    delta_pic_order_always_zero_flag: {} [{}] (1 bit)", sps.delta_pic_order_always_zero_flag as u8, sps.delta_pic_order_always_zero_flag));
                    lines.push(format!("    offset_for_non_ref_pic: {} (se)", sps.offset_for_non_ref_pic));
                    lines.push(format!("    offset_for_top_to_bottom_field: {} (se)", sps.offset_for_top_to_bottom_field));
                    lines.push(format!("    num_ref_frames_in_pic_order_cnt_cycle: {} (ue)", sps.num_ref_frames_in_pic_order_cnt_cycle));
                    lines.push(format!("    max_num_ref_frames: {} (ue)", sps.max_num_ref_frames));
                    lines.push(format!("    gaps_in_frame_num_value_allowed_flag: {} [{}] (1 bit)", sps.gaps_in_frame_num_value_allowed_flag as u8, sps.gaps_in_frame_num_value_allowed_flag));
                    lines.push(format!("    pic_width_in_mbs_minus1: {} (ue)", sps.pic_width_in_mbs_minus1));
                    lines.push(format!("    pic_height_in_map_units_minus1: {} (ue)", sps.pic_height_in_map_units_minus1));
                    lines.push(format!("    frame_mbs_only_flag: {} [{}] (1 bit)", sps.frame_mbs_only_flag as u8, sps.frame_mbs_only_flag));
                    lines.push(format!("    mb_adaptive_frame_field_flag: {} [{}] (1 bit)", sps.mb_adaptive_frame_field_flag as u8, sps.mb_adaptive_frame_field_flag));
                    lines.push(format!("    direct_8x8_inference_flag: {} [{}] (1 bit)", sps.direct_8x8_inference_flag as u8, sps.direct_8x8_inference_flag));
                    lines.push(format!("    frame_cropping_flag: {} [{}] (1 bit)", sps.frame_cropping_flag as u8, sps.frame_cropping_flag));
                    if sps.frame_cropping_flag {
                        lines.push(format!("    frame_crop_left_offset: {} (ue)", sps.frame_crop_left_offset));
                        lines.push(format!("    frame_crop_right_offset: {} (ue)", sps.frame_crop_right_offset));
                        lines.push(format!("    frame_crop_top_offset: {} (ue)", sps.frame_crop_top_offset));
                        lines.push(format!("    frame_crop_bottom_offset: {} (ue)", sps.frame_crop_bottom_offset));
                    }
                    lines.push(format!("    vui_parameters_present_flag: {} [{}] (1 bit)", sps.vui_parameters_present_flag as u8, sps.vui_parameters_present_flag));
                    lines.push(format!("    (derived) width: {}  height: {}", sps.width, sps.height));
                }
            }
            8 => {
                if let Some(pps) = parse_pps_rbsp(&rbsp) {
                    lines.push("  picture_parameter_set_rbsp():".to_string());
                    lines.push(format!("    pic_parameter_set_id: {} (ue)", pps.pic_parameter_set_id));
                    lines.push(format!("    seq_parameter_set_id: {} (ue)", pps.seq_parameter_set_id));
                    lines.push(format!("    entropy_coding_mode_flag: {} [{}] (1 bit)", pps.entropy_coding_mode_flag as u8, pps.entropy_coding_mode_flag));
                    lines.push(format!("    bottom_field_pic_order_in_frame_present_flag: {} [{}] (1 bit)", pps.bottom_field_pic_order_in_frame_present_flag as u8, pps.bottom_field_pic_order_in_frame_present_flag));
                    lines.push(format!("    num_slice_groups_minus1: {} (ue)", pps.num_slice_groups_minus1));
                    lines.push(format!("    slice_group_map_type: {} (ue)", pps.slice_group_map_type));
                    lines.push(format!("    num_ref_idx_l0_default_active_minus1: {} (ue)", pps.num_ref_idx_l0_default_active_minus1));
                    lines.push(format!("    num_ref_idx_l1_default_active_minus1: {} (ue)", pps.num_ref_idx_l1_default_active_minus1));
                    lines.push(format!("    weighted_pred_flag: {} [{}] (1 bit)", pps.weighted_pred_flag as u8, pps.weighted_pred_flag));
                    lines.push(format!("    weighted_bipred_idc: {} (ue)", pps.weighted_bipred_idc));
                    lines.push(format!("    pic_init_qp_minus26: {} (se)", pps.pic_init_qp_minus26));
                    lines.push(format!("    pic_init_qs_minus26: {} (se)", pps.pic_init_qs_minus26));
                    lines.push(format!("    chroma_qp_index_offset: {} (se)", pps.chroma_qp_index_offset));
                    lines.push(format!("    deblocking_filter_control_present_flag: {} [{}] (1 bit)", pps.deblocking_filter_control_present_flag as u8, pps.deblocking_filter_control_present_flag));
                    lines.push(format!("    constrained_intra_pred_flag: {} [{}] (1 bit)", pps.constrained_intra_pred_flag as u8, pps.constrained_intra_pred_flag));
                    lines.push(format!("    redundant_pic_cnt_present_flag: {} [{}] (1 bit)", pps.redundant_pic_cnt_present_flag as u8, pps.redundant_pic_cnt_present_flag));
                    lines.push(format!("    transform_8x8_mode_flag: {} [{}] (1 bit)", pps.transform_8x8_mode_flag as u8, pps.transform_8x8_mode_flag));
                    lines.push(format!("    pic_scaling_matrix_present_flag: {} [{}] (1 bit)", pps.pic_scaling_matrix_present_flag as u8, pps.pic_scaling_matrix_present_flag));
                    lines.push(format!("    second_chroma_qp_index_offset: {} (se)", pps.second_chroma_qp_index_offset));
                }
            }
            1 | 5 => {
                lines.push("  slice_layer_without_partitioning_rbsp():".to_string());
                lines.push("    slice_header():".to_string());
                if let Some((_, bit)) = read_ue(&rbsp, 0) {
                    if let Some((st, _)) = read_ue(&rbsp, bit) {
                        let st_base = st % 5;
                        let st_str = match st_base {
                            0 => "P",
                            1 => "B",
                            2 => "I",
                            3 => "Sp",
                            4 => "Si",
                            _ => "?",
                        };
                        lines.push(format!("      slice_type: {} ({}) (ue)", st, st_str));
                    }
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
