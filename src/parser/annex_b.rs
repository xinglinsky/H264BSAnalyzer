//! Annex B start code scanning: find NAL boundaries in a byte buffer.

/// One NAL span: start position, start code length, and data range (excluding start code).
#[derive(Debug, Clone)]
pub struct NalSpan {
    pub start_pos: u64,
    pub start_code_len: u8,
    pub data_start: u64,
    pub data_end: u64,
}

impl NalSpan {
    /// Total length in bytes (including start code).
    pub fn len(&self) -> u32 {
        (self.data_end - self.start_pos) as u32
    }
}

/// Scan buffer for Annex B start codes (0x00 0x00 0x01 or 0x00 0x00 0x00 0x01).
/// Returns a list of NAL spans. The buffer is not copied.
pub fn scan_nal_units(data: &[u8]) -> Vec<NalSpan> {
    let mut out = Vec::new();
    let mut i: usize = 0;
    while i + 3 <= data.len() {
        let start_code_len = if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            3
        } else if i + 4 <= data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            4
        } else {
            i += 1;
            continue;
        };
        let start_pos = i as u64;
        let data_start = (i + start_code_len) as u64;
        i += start_code_len;
        while i < data.len() {
            if i + 3 <= data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
                if i + 4 > data.len() || data[i + 3] != 0 {
                    break;
                }
            }
            if i + 4 <= data.len()
                && data[i] == 0
                && data[i + 1] == 0
                && data[i + 2] == 0
                && data[i + 3] == 1
            {
                break;
            }
            i += 1;
        }
        let data_end = i as u64;
        out.push(NalSpan {
            start_pos,
            start_code_len: start_code_len as u8,
            data_start,
            data_end,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_two_nal() {
        let data: Vec<u8> = [
            &[0u8, 0, 1][..],
            &[7u8, 0, 1][..],
            &[0, 0, 1][..],
            &[8u8][..],
        ]
        .concat();
        let spans = scan_nal_units(&data);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_code_len, 3);
        assert_eq!(spans[0].start_pos, 0);
        assert_eq!(spans[0].data_end - spans[0].data_start, 3);
        assert_eq!(spans[1].start_code_len, 3);
        assert_eq!(spans[1].data_end - spans[1].data_start, 1);
    }
}
