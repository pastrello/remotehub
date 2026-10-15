//! Bounded proof-of-concept framebuffer bridge for IronRDP -> Tauri WebView.
//! RGB images are encoded to JPEG on a separate worker and delivered with a session token.
//! Never store frames to disk or write them to diagnostic logs.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{codecs::jpeg::JpegEncoder, ExtendedColorType};
use serde::Serialize;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use tauri::{AppHandle, Emitter};

pub const MAX_PIXELS: usize = 2560 * 1600;
pub const MAX_FPS: u64 = 12;

pub struct RawFrame {
    pub sequence: u64,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FramePacket {
    event_id: String,
    sequence: u64,
    width: u16,
    height: u16,
    jpeg_base64: String,
}

pub fn bounded_frame_channel() -> (SyncSender<RawFrame>, Receiver<RawFrame>) {
    // At most one pending full framebuffer while the worker encodes/emits another.
    sync_channel(1)
}

pub fn encode_frame(frame: &RawFrame) -> Result<Vec<u8>, &'static str> {
    let pixel_count = usize::from(frame.width) * usize::from(frame.height);
    if pixel_count == 0 || pixel_count > MAX_PIXELS || frame.pixels.len() != pixel_count {
        return Err("Dimensões do framebuffer inválidas ou excessivas");
    }
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    for pixel in &frame.pixels {
        // IronRDP uses 0x00RRGGBB (red in bits 16..23).
        rgb.push((pixel >> 16) as u8);
        rgb.push((pixel >> 8) as u8);
        rgb.push(*pixel as u8);
    }
    let mut encoded = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut encoded, 76);
    encoder.encode(&rgb, u32::from(frame.width), u32::from(frame.height),
        ExtendedColorType::Rgb8).map_err(|_| "Falha ao codificar JPEG RDP")?;
    Ok(encoded)
}

pub fn process_frames(app: AppHandle, event_id: String, receiver: Receiver<RawFrame>) {
    while let Ok(frame) = receiver.recv() {
        if let Ok(jpeg) = encode_frame(&frame) {
            let packet = FramePacket {
                event_id: event_id.clone(),
                sequence: frame.sequence,
                width: frame.width,
                height: frame.height,
                jpeg_base64: STANDARD.encode(jpeg),
            };
            let _ = app.emit("rdp-frame", packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jpeg_encoding_is_valid() {
        let f = RawFrame { sequence: 1, width: 2, height: 2, pixels:
            vec![0x00ff0000, 0x0000ff00, 0x000000ff, 0x00ffffff] };
        let encoded = encode_frame(&f).unwrap();
        assert_eq!(&encoded[..3], &[0xff, 0xd8, 0xff]);
    }
    #[test]
    fn framebuffer_bounds_checked() {
        let f = RawFrame { sequence: 1, width: 2, height: 2, pixels: vec![0] };
        assert!(encode_frame(&f).is_err());
    }
}
