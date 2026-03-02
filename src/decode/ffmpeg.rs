//! FFmpeg-based decoder (compile with --features decode).

use std::ffi::CString;
use std::path::Path;

use rsmpeg::avcodec::AVCodecContext;
use rsmpeg::avformat::AVFormatContextInput;
use rsmpeg::avutil::AVFrame;
use rsmpeg::swscale::SwsContext;
use rsmpeg::avutil::AVPixelFormat;

use super::DecodedFrame;

pub struct Decoder {
    fmt_ctx: AVFormatContextInput,
    codec_ctx: AVCodecContext,
    stream_index: usize,
    sws: SwsContext,
    width: u32,
    height: u32,
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self, String> {
        let path_os = path.as_os_str().to_string_lossy();
        let c_path = CString::new(path_os.as_bytes()).map_err(|e| e.to_string())?;

        let mut fmt_ctx = AVFormatContextInput::open(c_path.as_c_str())
            .map_err(|e| format!("avformat open: {}", e))?;

        let (stream_index, codec) = fmt_ctx
            .find_best_stream(rsmpeg::avutil::AVMediaType::AVMEDIA_TYPE_VIDEO)
            .map_err(|e| format!("find stream: {}", e))?
            .ok_or_else(|| "no video stream".to_string())?;

        let stream = &fmt_ctx.streams()[stream_index];
        let mut codec_ctx = AVCodecContext::new(&codec);
        codec_ctx
            .apply_codecpar(stream.codecpar())
            .map_err(|e| format!("apply codecpar: {}", e))?;
        codec_ctx.open(None).map_err(|e| format!("codec open: {}", e))?;

        let width = codec_ctx.width as u32;
        let height = codec_ctx.height as u32;
        if width == 0 || height == 0 {
            return Err("invalid width/height".to_string());
        }

        let sws = SwsContext::get_context(
            width as i32,
            height as i32,
            codec_ctx.pix_fmt(),
            width as i32,
            height as i32,
            AVPixelFormat::AV_PIX_FMT_RGB24,
            rsmpeg::swscale::SwsFlags::SWS_BILINEAR,
        )
        .ok_or_else(|| "SwsContext::get_context failed".to_string())?;

        Ok(Decoder {
            fmt_ctx,
            codec_ctx,
            stream_index,
            sws,
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn next_frame(&mut self) -> Result<Option<DecodedFrame>, String> {
        loop {
            let packet = self
                .fmt_ctx
                .read_packet()
                .map_err(|e| format!("read_packet: {}", e))?;
            let packet = match packet {
                Some(p) => p,
                None => return Ok(None),
            };

            if packet.stream_index != self.stream_index as i32 {
                continue;
            }

            self.codec_ctx
                .send_packet(Some(&packet))
                .map_err(|e| format!("send_packet: {}", e))?;

            match self.codec_ctx.receive_frame() {
                Ok(frame) => return Ok(Some(self.frame_to_rgb(&frame)?)),
                Err(_) => continue,
            }
        }
    }

    pub fn flush_frames(&mut self) -> Result<Option<DecodedFrame>, String> {
        self.codec_ctx.send_packet(None).ok();
        match self.codec_ctx.receive_frame() {
            Ok(frame) => Ok(Some(self.frame_to_rgb(&frame)?)),
            Err(_) => Ok(None),
        }
    }

    fn frame_to_rgb(&self, frame: &AVFrame) -> Result<DecodedFrame, String> {
        let mut rgb_buffer = vec![0u8; (self.width * self.height * 3) as usize];
        let mut rgb_linesize = [(self.width * 3) as i32; 1];
        self.sws
            .scale(
                frame.data(),
                frame.linesize(),
                0,
                self.height as i32,
                &mut [rgb_buffer.as_mut_ptr()],
                &mut rgb_linesize,
            )
            .map_err(|e| format!("sws scale: {}", e))?;

        Ok(DecodedFrame {
            width: self.width,
            height: self.height,
            rgb: rgb_buffer,
        })
    }
}
