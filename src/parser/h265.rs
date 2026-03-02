//! H.265/HEVC file parsing: NAL list (scan + type), NAL detail tree, and optional SPS/PPS for file info.

use std::fs;
use std::path::Path;

use crate::model::{FileType, NaluInfo, NalUnitType, ParseResult, SliceType, SpsInfo, PpsInfo};
use crate::parser::annex_b::scan_nal_units;

/// Parse H.265 Annex B file: NAL list with type from header.
pub fn parse_h265_file(path: &Path) -> std::io::Result<ParseResult> {
    let data = fs::read(path)?;
    parse_h265_bytes(&data)
}

/// Skip profile_tier_level() syntax in HEVC SPS/VPS.
fn skip_profile_tier_level(br: &mut BitReader, max_sub_layers_minus1: u8) -> Option<(u8, u8)> {
    let _general_profile_space = br.read_bits(2)?;
    let _general_tier_flag = br.read_bit()?;
    let general_profile_idc = br.read_bits(5)? as u8;
    let _general_profile_compatibility_flags = br.read_bits(32)?;
    // general_progressive_source_flag, general_interlaced_source_flag,
    // general_non_packed_constraint_flag, general_frame_only_constraint_flag
    // + general_reserved_zero_44bits => total 48 bits
    let _general_constraint_flags = br.read_bits(4)?;
    let _general_reserved_zero_44_hi = br.read_bits(32)?;
    let _general_reserved_zero_44_lo = br.read_bits(12)?;
    let general_level_idc = br.read_bits(8)? as u8;

    let mut sub_layer_profile_present_flag = [0u8; 8];
    let mut sub_layer_level_present_flag = [0u8; 8];
    for i in 0..max_sub_layers_minus1 as usize {
        sub_layer_profile_present_flag[i] = br.read_bit()?;
        sub_layer_level_present_flag[i] = br.read_bit()?;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1 as usize..8 {
            let _reserved_zero_2bits = br.read_bits(2)?;
        }
    }

    for i in 0..max_sub_layers_minus1 as usize {
        if sub_layer_profile_present_flag[i] != 0 {
            let _sub_layer_profile_space = br.read_bits(2)?;
            let _sub_layer_tier_flag = br.read_bit()?;
            let _sub_layer_profile_idc = br.read_bits(5)?;
            let _sub_layer_profile_compatibility_flags = br.read_bits(32)?;
            let _sub_layer_constraint_flags = br.read_bits(4)?;
            let _sub_layer_reserved_zero_44_hi = br.read_bits(32)?;
            let _sub_layer_reserved_zero_44_lo = br.read_bits(12)?;
        }
        if sub_layer_level_present_flag[i] != 0 {
            let _sub_layer_level_idc = br.read_bits(8)?;
        }
    }

    Some((general_profile_idc, general_level_idc))
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
    let (sps_info, pps_info) = extract_h265_sps_pps(data, &spans);
    Ok(ParseResult {
        file_type: FileType::H265,
        nalus,
        sps_info,
        pps_info,
    })
}

/// EBSP to RBSP: remove 0x03 stuffing after 0x00 0x00.
fn ebsp_to_rbsp(ebsp: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(ebsp.len());
    let mut i = 0;
    while i < ebsp.len() {
        if i + 3 <= ebsp.len() && ebsp[i] == 0 && ebsp[i + 1] == 0 && ebsp[i + 2] == 3 {
            rbsp.extend_from_slice(&ebsp[i..i + 2]);
            i += 3;
        } else {
            rbsp.push(ebsp[i]);
            i += 1;
        }
    }
    rbsp
}

/// Bit reader over bytes (MSB first).
struct BitReader<'a> {
    data: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
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
        let v = self.read_ue()?;
        let k = (v as i64) & 1;
        Some(((v as i64) + 1 >> 1) as i32 * (1 - 2 * k as i32))
    }
    #[allow(dead_code)]
    fn bit_offset(&self) -> usize {
        self.bit_offset
    }
}

/// NAL unit type description for H.265 (for NAL list and display).
pub fn h265_nal_type_name(t: u8) -> &'static str {
    match t {
        0 => "Coded slice segment of a TRAIL_R picture",
        1 => "Coded slice segment of a non-TSA, non-STSA picture",
        2 => "Coded slice segment of a TSA picture",
        3 => "Coded slice segment of an STSA picture",
        4 => "Coded slice segment of a RADL picture",
        5 => "Coded slice segment of a RASL picture",
        6 => "Coded slice segment of a BLA picture",
        7 => "Coded slice segment of an IDR picture",
        8 => "Coded slice segment of a CRA picture",
        9 => "Coded slice segment of an IDR picture",
        16 => "Coded slice segment of a BLA picture",
        17 => "Coded slice segment of a BLA picture",
        18 => "Coded slice segment of an IDR picture",
        19 => "Coded slice segment of an IDR picture",
        20 => "Coded slice segment of a CRA picture",
        32 => "Video parameter set",
        33 => "Sequence parameter set",
        34 => "Picture parameter set",
        35 => "Access unit delimiter",
        36 => "End of sequence",
        37 => "End of bitstream",
        38 => "Filler data",
        39 => "Supplemental enhancement info",
        40..=47 => "Reserved",
        _ => "NAL unit",
    }
}

/// Build tree/detail text for H.265 NAL (for right panel), matching original style with field names and bit lengths.
pub fn tree_text_for_nal_h265(nal_raw: &[u8]) -> String {
    if nal_raw.len() < 5 {
        return format!("Invalid NAL (len={})", nal_raw.len());
    }
    let (_start_code_len, header_start) = if nal_raw.starts_with(&[0, 0, 0, 1]) {
        (4, 4)
    } else if nal_raw.starts_with(&[0, 0, 1]) {
        (3, 3)
    } else {
        return "No Annex B start code".to_string();
    };
    if header_start + 2 > nal_raw.len() {
        return "NAL header too short".to_string();
    }
    let b0 = nal_raw[header_start];
    let b1 = nal_raw[header_start + 1];
    let forbidden_zero_bit = (b0 >> 7) & 1;
    let nal_unit_type = (b0 >> 1) & 0x3F;
    let nuh_layer_id = (b1 >> 3) & 0x3F;
    let nuh_temporal_id_plus1 = b1 & 7;

    let mut lines: Vec<String> = vec![
        "NAL".to_string(),
        "  nal_unit_header".to_string(),
        format!("    forbidden_zero_bit: {} (1 bit)", forbidden_zero_bit),
        format!(
            "    nal_unit_type: {} ({})(6 bits)",
            nal_unit_type,
            h265_nal_type_name(nal_unit_type)
        ),
        format!("    nuh_layer_id: {} (6 bits)", nuh_layer_id),
        format!("    nuh_temporal_id_plus1: {} (3 bits)", nuh_temporal_id_plus1),
    ];

    let is_slice = matches!(nal_unit_type, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 16 | 17 | 18 | 19 | 20);
    if nal_unit_type == 32 && header_start + 2 < nal_raw.len() {
        let ebsp = &nal_raw[header_start + 2..];
        let rbsp = ebsp_to_rbsp(ebsp);
        let mut br = BitReader::new(&rbsp);
        lines.push("  video_parameter_set_rbsp():".to_string());
        if let Some(v) = br.read_ue() {
            lines.push(format!("    vps_video_parameter_set_id: {} (ue)", v));
        }
        let _reserved = br.read_bits(2);
        if let Some(v) = br.read_ue() {
            lines.push(format!("    vps_max_layers_minus1: {} (ue)", v));
        }
        if let Some(max_sub_layers) = br.read_bits(3) {
            let max_sub_layers = max_sub_layers as u8;
            lines.push(format!("    vps_max_sub_layers_minus1: {} (3 bits)", max_sub_layers));
            if let Some(f) = br.read_bit() {
                lines.push(format!("    vps_temporal_id_nesting_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
            }
            lines.push("    profile_tier_level(1, vps_max_sub_layers_minus1):".to_string());
            let _ps = br.read_bits(2);
            let _tier = br.read_bit();
            if let Some(p) = br.read_bits(8) {
                lines.push(format!("      general_profile_idc: {} (8 bits)", p));
            }
            let _ = br.read_bits(32);
            if let Some(l) = br.read_bits(8) {
                lines.push(format!("      general_level_idc: {} (8 bits)", l));
            }
            for _ in 0..max_sub_layers {
                let _fp = br.read_bit();
                let _fl = br.read_bit();
                if br.read_bit().unwrap_or(0) != 0 {
                    let _ = br.read_bits(2 + 1 + 8 + 32);
                }
                if br.read_bit().unwrap_or(0) != 0 {
                    let _ = br.read_bits(8);
                }
            }
            if let Some(f) = br.read_bit() {
                lines.push(format!("    vps_sub_layer_ordering_info_present_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    vps_max_dec_pic_buffering_minus1[0]: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    vps_max_num_reorder_pics[0]: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    vps_max_latency_increase_plus1[0]: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    vps_max_layer_id: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    vps_num_layer_sets_minus1: {} (ue)", v));
            }
        }
    } else if nal_unit_type == 33 && header_start + 2 < nal_raw.len() {
        let ebsp = &nal_raw[header_start + 2..];
        let rbsp = ebsp_to_rbsp(ebsp);
        let mut br = BitReader::new(&rbsp);
        lines.push("  sequence_parameter_set_rbsp():".to_string());
        if let Some(v) = br.read_ue() {
            lines.push(format!("    sps_video_parameter_set_id: {} (ue)", v));
        }
        if let Some(max_sub_layers) = br.read_bits(3) {
            let max_sub_layers = max_sub_layers as u8;
            lines.push(format!("    sps_max_sub_layers_minus1: {} (3 bits)", max_sub_layers));
            if let Some(f) = br.read_bit() {
                lines.push(format!("    sps_temporal_id_nesting_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
            }
            lines.push("    profile_tier_level(1, sps_max_sub_layers_minus1):".to_string());
            let _ps = br.read_bits(2);
            let _tier = br.read_bit();
            if let Some(p) = br.read_bits(8) {
                lines.push(format!("      general_profile_idc: {} (8 bits)", p));
            }
            let _ = br.read_bits(32);
            if let Some(l) = br.read_bits(8) {
                lines.push(format!("      general_level_idc: {} (8 bits)", l));
            }
            for _ in 0..max_sub_layers {
                let _fp = br.read_bit();
                let _fl = br.read_bit();
                if br.read_bit().unwrap_or(0) != 0 {
                    let _ = br.read_bits(2 + 1 + 8 + 32);
                }
                if br.read_bit().unwrap_or(0) != 0 {
                    let _ = br.read_bits(8);
                }
            }
            if let Some(sps_id) = br.read_ue() {
                lines.push(format!("    sps_seq_parameter_set_id: {} (ue)", sps_id));
            }
            if let Some(c) = br.read_ue() {
                lines.push(format!("    chroma_format_idc: {} (ue)", c));
                if c == 3 {
                    let _ = br.read_bit();
                }
            }
            if let Some(w) = br.read_ue() {
                lines.push(format!("    pic_width_in_luma_samples: {} (ue)", w));
            }
            if let Some(h) = br.read_ue() {
                lines.push(format!("    pic_height_in_luma_samples: {} (ue)", h));
            }
            if let Some(f) = br.read_bit() {
                lines.push(format!("    conformance_window_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
                if f != 0 {
                    if let Some(v) = br.read_ue() {
                        lines.push(format!("    conf_win_left_offset: {} (ue)", v));
                    }
                    if let Some(v) = br.read_ue() {
                        lines.push(format!("    conf_win_right_offset: {} (ue)", v));
                    }
                    if let Some(v) = br.read_ue() {
                        lines.push(format!("    conf_win_top_offset: {} (ue)", v));
                    }
                    if let Some(v) = br.read_ue() {
                        lines.push(format!("    conf_win_bottom_offset: {} (ue)", v));
                    }
                }
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    bit_depth_luma_minus8: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    bit_depth_chroma_minus8: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    log2_max_pic_order_cnt_lsb_minus4: {} (ue)", v));
            }
            if let Some(f) = br.read_bit() {
                lines.push(format!("    sps_sub_layer_ordering_info_present_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    sps_max_dec_pic_buffering_minus1[0]: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    sps_max_num_reorder_pics[0]: {} (ue)", v));
            }
            if let Some(v) = br.read_ue() {
                lines.push(format!("    sps_max_latency_increase_plus1[0]: {} (ue)", v));
            }
        }
    } else if nal_unit_type == 34 && header_start + 2 < nal_raw.len() {
        let ebsp = &nal_raw[header_start + 2..];
        let rbsp = ebsp_to_rbsp(ebsp);
        let mut br = BitReader::new(&rbsp);
        lines.push("  picture_parameter_set_rbsp():".to_string());
        if let Some(v) = br.read_ue() {
            lines.push(format!("    pps_pic_parameter_set_id: {} (ue)", v));
        }
        if let Some(v) = br.read_ue() {
            lines.push(format!("    pps_seq_parameter_set_id: {} (ue)", v));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    dependent_slice_segments_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    output_flag_present_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(v) = br.read_bits(3) {
            lines.push(format!("    num_extra_slice_header_bits: {} (3 bits)", v));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    sign_data_hiding_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    cabac_init_present_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(v) = br.read_ue() {
            lines.push(format!("    num_ref_idx_l0_default_active_minus1: {} (ue)", v));
        }
        if let Some(v) = br.read_ue() {
            lines.push(format!("    num_ref_idx_l1_default_active_minus1: {} (ue)", v));
        }
        if let Some(v) = br.read_se() {
            lines.push(format!("    init_qp_minus26: {} (se)", v));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    constrained_intra_pred_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    transform_skip_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    cu_qp_delta_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(v) = br.read_se() {
            lines.push(format!("    pps_cb_qp_offset: {} (se)", v));
        }
        if let Some(v) = br.read_se() {
            lines.push(format!("    pps_cr_qp_offset: {} (se)", v));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    pps_slice_chroma_qp_offsets_present_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    weighted_pred_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(v) = br.read_bits(2) {
            lines.push(format!("    weighted_bipred_idc: {} (2 bits)", v));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    transquant_bypass_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    tiles_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
        if let Some(f) = br.read_bit() {
            lines.push(format!("    entropy_coding_sync_enabled_flag: {} [{}] (1 bit)", f, if f != 0 { "True" } else { "False" }));
        }
    } else if is_slice && header_start + 2 < nal_raw.len() {
        lines.push("  slice_segment_layer_rbsp()".to_string());
        let ebsp = &nal_raw[header_start + 2..];
        let rbsp = ebsp_to_rbsp(ebsp);
        let mut br = BitReader::new(&rbsp);
        lines.push("    slice_segment_header()".to_string());

        let first_slice = br.read_bit();
        if let Some(f) = first_slice {
            lines.push(format!(
                "      first_slice_segment_in_pic_flag: {} [{}] (1 bit)",
                f,
                if f != 0 { "True" } else { "False" }
            ));
        }
        let is_idr = matches!(nal_unit_type, 7 | 8 | 9 | 16 | 17 | 18 | 19 | 20);
        if is_idr {
            if let Some(f) = br.read_bit() {
                lines.push(format!(
                    "      no_output_of_prior_pics_flag: {} [{}] (1 bit)",
                    f,
                    if f != 0 { "True" } else { "False" }
                ));
            }
        }
        if let Some(pps_id) = br.read_ue() {
            lines.push(format!("      slice_pic_parameter_set_id: {} (v bits)", pps_id));
        }
        fn parse_slice_type_sao(
            br: &mut BitReader,
            lines: &mut Vec<String>,
        ) {
            if let Some(slice_type) = br.read_ue() {
                let st_str = match slice_type {
                    0 => "B",
                    1 => "P",
                    2 => "I",
                    _ => "?",
                };
                lines.push(format!("      slice_type: {} ({})(v bits)", slice_type, st_str));
            }
            let _pic_output = br.read_bit();
            let _num_ref_override = br.read_bit();
            if let Some(sao_luma) = br.read_bit() {
                lines.push(format!(
                    "      slice_sao_luma_flag: {} [{}] (1 bit)",
                    sao_luma,
                    if sao_luma != 0 { "True" } else { "False" }
                ));
            }
            if let Some(sao_chroma) = br.read_bit() {
                lines.push(format!(
                    "      slice_sao_chroma_flag: {} [{}] (1 bit)",
                    sao_chroma,
                    if sao_chroma != 0 { "True" } else { "False" }
                ));
            }
            if let Some(qp_delta) = br.read_se() {
                lines.push(format!("      slice_qp_delta: {} (v bits)", qp_delta));
            }
            if let Some(lf_enabled) = br.read_bit() {
                lines.push(format!(
                    "      slice_loop_filter_across_slices_enabled_flag: {} [{}] (1 bit)",
                    lf_enabled,
                    if lf_enabled != 0 { "True" } else { "False" }
                ));
            }
        }
        if let Some(first) = first_slice {
            if first == 0 {
                if let Some(dep) = br.read_bit() {
                    lines.push(format!(
                        "      dependent_slice_segment_flag: {} [{}] (1 bit)",
                        dep,
                        if dep != 0 { "True" } else { "False" }
                    ));
                    if dep == 0 {
                        parse_slice_type_sao(&mut br, &mut lines);
                    }
                }
            } else {
                parse_slice_type_sao(&mut br, &mut lines);
            }
        }
        lines.push("    slice_segment_data()".to_string());
        lines.push("    rbsp_slice_segment_trailing_bits()".to_string());
    } else {
        lines.push("    (non-slice or no payload)".to_string());
    }

    lines.push(format!("  Length: {} bytes", nal_raw.len()));
    lines.join("\n")
}

/// Minimal H.265 SPS/PPS extraction for file info (picture size, crop, profile, level).
fn extract_h265_sps_pps(data: &[u8], spans: &[crate::parser::annex_b::NalSpan]) -> (Option<SpsInfo>, Option<PpsInfo>) {
    fn level_idc_known(level_idc: u8) -> bool {
        matches!(
            level_idc,
            30 | 60 | 63 | 90 | 93 | 111 | 120 | 123 | 126 | 153 | 156 | 159 | 162 | 180 | 183 | 186 | 189 | 192
        )
    }

    fn sps_score(s: &SpsInfo) -> i64 {
        let mut score = 0i64;
        if s.width > 0 && s.height > 0 {
            score += 1000;
        }
        // common video sizes upper bound, avoid absurd parse results
        if s.width <= 16384 && s.height <= 16384 {
            score += 200;
        }
        if level_idc_known(s.level_idc) {
            score += 100;
        }
        // Common HEVC profiles in this tool: Main/Main10/Main Still Picture
        if matches!(s.profile_idc, 1 | 2 | 3) {
            score += 50;
        }
        // prefer larger resolution if all else equal
        score + (s.width as i64) + (s.height as i64)
    }

    let mut best_sps: Option<SpsInfo> = None;
    let mut best_sps_score = i64::MIN;
    let mut pps_info = None;
    for span in spans {
        let data_start = span.data_start as usize;
        let data_end = span.data_end as usize;
        if data_end > data.len() || data_start + 2 > data.len() {
            continue;
        }
        let nal_type = (data[data_start] >> 1) & 0x3F;
        if nal_type == 33 {
            if let Some(s) = parse_h265_sps_rbsp(data, data_start, data_end) {
                let score = sps_score(&s);
                if score > best_sps_score {
                    best_sps_score = score;
                    best_sps = Some(s);
                }
            }
        }
        if nal_type == 34 && pps_info.is_none() {
            pps_info = Some(PpsInfo {
                entropy_coding_mode_flag: false,
            });
        }
    }
    (best_sps, pps_info)
}

/// Minimal parse of H.265 SPS NAL to get width, height, crop, profile_idc, level_idc.
fn parse_h265_sps_rbsp(data: &[u8], data_start: usize, data_end: usize) -> Option<SpsInfo> {
    if data_start + 2 > data_end {
        return None;
    }
    let ebsp = &data[data_start + 2..data_end];
    let rbsp = ebsp_to_rbsp(ebsp);
    let mut br = BitReader::new(&rbsp);
    let _sps_video_parameter_set_id = br.read_bits(4)?;
    let sps_max_sub_layers_minus1 = br.read_bits(3)? as u8;
    let _temporal_id_nesting = br.read_bit()?;
    let (profile_idc, level_idc) = skip_profile_tier_level(&mut br, sps_max_sub_layers_minus1)?;
    let _sps_id = br.read_ue()?;
    let chroma_format_idc = br.read_ue()? as u8;
    let separate_colour_plane_flag = if chroma_format_idc == 3 { br.read_bit()? } else { 0 };
    let pic_width = br.read_ue()? as u32;
    let pic_height = br.read_ue()? as u32;
    let conformance_window_flag = br.read_bit()? != 0;
    let (sub_width_c, sub_height_c) = match chroma_format_idc {
        0 => (1u32, 1u32),
        1 => (2u32, 2u32),
        2 => (2u32, 1u32),
        3 => {
            if separate_colour_plane_flag != 0 {
                (1u32, 1u32)
            } else {
                (1u32, 1u32)
            }
        }
        _ => (1u32, 1u32),
    };
    let (crop_left, crop_right, crop_top, crop_bottom, crop_left_px, crop_right_px, crop_top_px, crop_bottom_px) = if conformance_window_flag {
        let l = br.read_ue()? as u32;
        let r = br.read_ue()? as u32;
        let t = br.read_ue()? as u32;
        let b = br.read_ue()? as u32;
        (
            l,
            r,
            t,
            b,
            l.saturating_mul(sub_width_c),
            r.saturating_mul(sub_width_c),
            t.saturating_mul(sub_height_c),
            b.saturating_mul(sub_height_c),
        )
    } else {
        (0u32, 0, 0, 0, 0, 0, 0, 0)
    };
    let width = pic_width.saturating_sub(crop_left_px).saturating_sub(crop_right_px);
    let height = pic_height.saturating_sub(crop_top_px).saturating_sub(crop_bottom_px);
    Some(SpsInfo {
        profile_idc,
        level_idc,
        width,
        height,
        crop_left,
        crop_right,
        crop_top,
        crop_bottom,
        max_framerate: 30.0,
        chroma_format_idc,
    })
}
