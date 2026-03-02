//! Data models equivalent to original NALU_t, SPSInfo_t, FileType.

use std::path::Path;

/// File format type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileType {
    #[default]
    Unknown,
    H264,
    H265,
}

/// NAL unit type (H.264).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalUnitType {
    Unspecified,
    NonIdrSlice,
    DataPartitionA,
    DataPartitionB,
    DataPartitionC,
    IdrSlice,
    Sei,
    Sps,
    Pps,
    Aud,
    EndOfSeq,
    EndOfStream,
    Filler,
    SpsExt,
    Prefix,
    SubsetSps,
    DepthParameterSet,
    Reserved(u8),
    UnspecifiedExt(u8),
}

impl From<u8> for NalUnitType {
    fn from(v: u8) -> Self {
        use NalUnitType::*;
        match v {
            0 => Unspecified,
            1 => NonIdrSlice,
            2 => DataPartitionA,
            3 => DataPartitionB,
            4 => DataPartitionC,
            5 => IdrSlice,
            6 => Sei,
            7 => Sps,
            8 => Pps,
            9 => Aud,
            10 => EndOfSeq,
            11 => EndOfStream,
            12 => Filler,
            13 => SpsExt,
            14 => Prefix,
            15 => SubsetSps,
            16 => DepthParameterSet,
            17..=18 => Reserved(v),
            19..=23 => UnspecifiedExt(v),
            _ => Unspecified,
        }
    }
}

impl std::fmt::Display for NalUnitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NalUnitType::Unspecified => "Unspecified",
            NalUnitType::NonIdrSlice => "Non-IDR Slice",
            NalUnitType::DataPartitionA => "Data Partition A",
            NalUnitType::DataPartitionB => "Data Partition B",
            NalUnitType::DataPartitionC => "Data Partition C",
            NalUnitType::IdrSlice => "IDR Slice",
            NalUnitType::Sei => "SEI",
            NalUnitType::Sps => "SPS",
            NalUnitType::Pps => "PPS",
            NalUnitType::Aud => "AUD",
            NalUnitType::EndOfSeq => "End of Seq",
            NalUnitType::EndOfStream => "End of Stream",
            NalUnitType::Filler => "Filler",
            NalUnitType::SpsExt => "SPS Ext",
            NalUnitType::Prefix => "Prefix",
            NalUnitType::SubsetSps => "Subset SPS",
            NalUnitType::DepthParameterSet => "Depth Param",
            NalUnitType::Reserved(n) => return write!(f, "Reserved({})", n),
            NalUnitType::UnspecifiedExt(n) => return write!(f, "UnspecifiedExt({})", n),
        };
        f.write_str(s)
    }
}

/// Slice type for slice NALs (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliceType {
    #[default]
    Unknown,
    P,
    B,
    I,
}

/// One NAL unit entry (equivalent to NALU_t).
#[derive(Debug, Clone)]
pub struct NaluInfo {
    pub index: u32,
    pub offset: u64,
    pub len: u32,
    pub start_code_len: u8,
    pub nal_type: NalUnitType,
    /// H.265 NAL unit type (0..63) when file is H.265.
    pub h265_nal_type: Option<u8>,
    pub slice_type: SliceType,
    /// Raw bytes (start code + NAL), for hex view.
    pub raw: Vec<u8>,
}

/// SPS-derived video info (equivalent to SPSInfo_t).
#[derive(Debug, Clone, Default)]
pub struct SpsInfo {
    pub profile_idc: u8,
    pub level_idc: u8,
    pub width: u32,
    pub height: u32,
    pub crop_left: u32,
    pub crop_right: u32,
    pub crop_top: u32,
    pub crop_bottom: u32,
    pub max_framerate: f32,
    pub chroma_format_idc: u8,
}

/// PPS-derived info (equivalent to PPSInfo_t).
#[derive(Debug, Clone, Default)]
pub struct PpsInfo {
    pub entropy_coding_mode_flag: bool, // true = CABAC, false = CAVLC
}

/// Result of parsing a file: NAL list + optional SPS/PPS summary.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub file_type: FileType,
    pub nalus: Vec<NaluInfo>,
    pub sps_info: Option<SpsInfo>,
    pub pps_info: Option<PpsInfo>,
}

/// Guess file type from path (extension) and/or content.
pub fn guess_file_type(path: &Path, prefix: Option<&[u8]>) -> FileType {
    let ext = path.extension().and_then(|e| e.to_str());
    let is_h264 = ext.map_or(false, |e| {
        e.eq_ignore_ascii_case("h264") || e.eq_ignore_ascii_case("264") || e.eq_ignore_ascii_case("avc")
    });
    let is_h265 = ext.map_or(false, |e| {
        e.eq_ignore_ascii_case("h265") || e.eq_ignore_ascii_case("265") || e.eq_ignore_ascii_case("hevc")
    });
    if is_h264 && !is_h265 {
        return FileType::H264;
    }
    if is_h265 && !is_h264 {
        return FileType::H265;
    }
    if let Some(p) = prefix {
        if p.len() >= 4 {
            if p[0] == 0 && p[1] == 0 && p[2] == 1 {
                let b = if p.len() > 4 { p[4] } else { 0 };
                let t = b & 0x1F;
                if t == 7 {
                    return FileType::H264;
                }
                if t == 32 || t == 33 || t == 34 {
                    return FileType::H265;
                }
            }
            if p.len() >= 5 && p[0] == 0 && p[1] == 0 && p[2] == 0 && p[3] == 1 {
                let b = if p.len() > 5 { p[5] } else { 0 };
                let t = (b >> 1) & 0x3F;
                if t == 7 {
                    return FileType::H264;
                }
                if t == 32 || t == 33 || t == 34 {
                    return FileType::H265;
                }
            }
        }
    }
    FileType::Unknown
}
