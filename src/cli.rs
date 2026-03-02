//! CLI mode: parse file and print summary to stdout.

use std::path::Path;

use crate::parser::{h265_nal_type_name, parse_file};
use crate::model::*;

pub fn run(path: &Path) -> std::io::Result<()> {
    let result = parse_file(path)?;
    let codec = match result.file_type {
        FileType::H264 => "H.264/AVC",
        FileType::H265 => "H.265/HEVC",
        _ => "Unknown",
    };
    println!("File: {}", path.display());
    println!("Codec: {}", codec);
    if let Some(ref s) = result.sps_info {
        println!("Picture Size: {}x{}", s.width, s.height);
        println!(
            "Cropping: L{} R{} T{} B{}",
            s.crop_left, s.crop_right, s.crop_top, s.crop_bottom
        );
        println!("Profile: {}  Level: {}", s.profile_idc, s.level_idc);
    }
    if let Some(ref p) = result.pps_info {
        let enc = if p.entropy_coding_mode_flag {
            "CABAC"
        } else {
            "CAVLC"
        };
        println!("Encoding: {}", enc);
    }
    println!("NAL count: {}", result.nalus.len());
    println!();
    println!("{:>4}  {:>10}  {:>8}  {}", "No.", "Offset", "Length", "Type");
    println!("{}", "-".repeat(40));
    for n in &result.nalus {
        let ty = match result.file_type {
            FileType::H265 => {
                n.h265_nal_type
                    .map(h265_nal_type_name)
                    .unwrap_or("?")
                    .to_string()
            }
            _ => format!("{}", n.nal_type),
        };
        println!("{:>4}  {:>10}  {:>8}  {}", n.index, n.offset, n.len, ty);
    }
    Ok(())
}
