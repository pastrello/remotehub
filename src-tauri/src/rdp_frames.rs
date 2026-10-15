//! Bounded JPEG delta bridge. Never persist remote screen data to disk.
//! IronRDP's ordered updates are composed *before* this lossy presentation layer.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{codecs::jpeg::JpegEncoder, ExtendedColorType};
use serde::Serialize;
use std::{
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

pub const MAX_PIXELS: usize = 2560 * 1600;
pub const FRAME_INTERVAL_MS: u64 = 40; // 25 FPS ceiling, ACK limits actual rate.
pub const ACK_TIMEOUT_MS: u64 = 1500;

pub struct RawFrame {
    pub sequence:u64,
    pub full_width:u16,
    pub full_height:u16,
    pub x:u16,
    pub y:u16,
    pub width:u16,
    pub height:u16,
    pub pixels:Vec<u32>,
}
#[derive(Serialize, Clone)]
#[serde(rename_all="camelCase")]
struct FramePacket {
    event_id:String,
    sequence:u64,
    full_width:u16,
    full_height:u16,
    x:u16,
    y:u16,
    width:u16,
    height:u16,
    jpeg_base64:String,
    jpeg_bytes:usize,
    encode_ms:u64,
    sent_at_ms:u64,
}
pub fn bounded_frame_channel() -> (SyncSender<RawFrame>, Receiver<RawFrame>) {
    sync_channel(1)
}
pub fn encode_frame(frame:&RawFrame) -> Result<Vec<u8>,&'static str> {
    let n=usize::from(frame.width)*usize::from(frame.height);
    if n==0 || n>MAX_PIXELS || frame.pixels.len()!=n ||
       frame.full_width==0 || frame.full_height==0 ||
       usize::from(frame.x)+usize::from(frame.width)>usize::from(frame.full_width) ||
       usize::from(frame.y)+usize::from(frame.height)>usize::from(frame.full_height)
    {return Err("Região JPEG inválida");}
    let mut rgb=Vec::with_capacity(n*3);
    for pixel in &frame.pixels {
        rgb.push((pixel>>16) as u8);
        rgb.push((pixel>>8) as u8);
        rgb.push(*pixel as u8);
    }
    let mut encoded=Vec::new();
    JpegEncoder::new_with_quality(&mut encoded,78)
        .encode(&rgb,u32::from(frame.width),u32::from(frame.height),ExtendedColorType::Rgb8)
        .map_err(|_|"Falha ao codificar atualização JPEG")?;
    Ok(encoded)
}
pub fn process_frames(app:AppHandle,event_id:String,receiver:Receiver<RawFrame>) {
    while let Ok(frame)=receiver.recv() {
        let begun=Instant::now();
        if let Ok(jpeg)=encode_frame(&frame) {
            let packet=FramePacket {
                event_id:event_id.clone(),sequence:frame.sequence,
                full_width:frame.full_width,full_height:frame.full_height,
                x:frame.x,y:frame.y,width:frame.width,height:frame.height,
                jpeg_base64:STANDARD.encode(&jpeg),
                jpeg_bytes:jpeg.len(),
                encode_ms:begun.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                sent_at_ms:SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64,
            };
            let _=app.emit("rdp-frame",packet);
        }
        // If emit/encoding fails the caller will recover via ACK timeout + full refresh.
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample()->RawFrame {RawFrame {
        sequence:1,full_width:4,full_height:4,x:1,y:1,width:2,height:2,
        pixels:vec![0x00ff0000,0x0000ff00,0x000000ff,0x00ffffff]
    }}
    #[test]
    fn jpeg_delta_encoding_is_valid() {
        let encoded=encode_frame(&sample()).unwrap();
        assert_eq!(&encoded[..3],&[0xff,0xd8,0xff]);
    }
    #[test]
    fn malformed_patch_is_rejected() {
        let mut f=sample();
        f.x=3;
        assert!(encode_frame(&f).is_err());
        f.x=1;f.pixels.clear();
        assert!(encode_frame(&f).is_err());
    }
}
