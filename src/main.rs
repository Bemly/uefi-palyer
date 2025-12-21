#![no_main]
#![no_std]

extern crate alloc;

mod fs;
mod graphics;
mod error;
mod video;

use uefi::prelude::*;
use uefi::proto::console::gop::BltPixel;
use core::time::Duration;
use uefi::{println, CStr16};
use uefi::proto::media::file::RegularFile;
use crate::error::{handle_fatal, Result};
use crate::fs::Fs;
use crate::graphics::Screen;
use crate::video::buffer::{BltFrameBuffer, QoiFrameBuffer, RawFrameBuffer};

#[entry]
fn main() -> Status {
    run().unwrap_or_else(|e| handle_fatal(e));
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}

fn run() -> Result {
    uefi::helpers::init()?;


    let mut fs = Fs::new()?;
    let mut screen = Screen::new()?;
    let mut file = fs.open_file(cstr16!("anime\\video.qois"))?;
    let mut qoi = QoiFrameBuffer::new(1280 * 720);
    let mut raw = RawFrameBuffer::new(1280 * 720);
    let mut blt = BltFrameBuffer::new(1280 * 720);
    loop {
        draw(&mut fs, &mut file, &mut screen, &mut qoi, &mut raw, &mut blt)?;
        boot::stall(Duration::from_millis(1000 / 90));
    }


    // Ok(())
}

fn draw_once(fs: &mut Fs, screen: &mut Screen, path: &CStr16, blt_buf: &mut [BltPixel]) -> Result {
    // 加载文件
    let qoi_data = fs.read_file(path)?;

    // 解码图像
    let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)?;

    // 转换像素格式
    blt_buf.iter_mut()
        .zip(rgba_buf.chunks_exact(header.channels.as_u8() as usize))
        .for_each(|(dest, src)| {
            *dest = BltPixel::new(src[0], src[1], src[2]);
        });

    // 渲染
    screen.draw_image(header.width, header.height, &blt_buf)?;

    Ok(())
}

fn draw(
    fs: &mut Fs,
    file: &mut RegularFile,
    screen: &mut Screen,
    qoi: &mut QoiFrameBuffer,
    raw: &mut RawFrameBuffer,
    blt: &mut BltFrameBuffer
) -> Result {
    // 读文件流
    if fs.read_frame_next(file, &mut qoi.0)? {
        // 解码
        raw.header = loop {
            match qoi::decode_to_buf(&mut raw.pixels, &qoi.0) {
                Ok(header) => break header,
                Err(qoi::Error::OutputBufferTooSmall { required, .. }) => {
                    raw.pixels.resize(required, 0)
                }
                Err(e) => Err(e)?,
            }
        };

        // 转换
        // 只取当前帧需要的切片范围，避免处理旧数据
        let step = raw.header.channels.as_u8() as usize;
        let raw_data = raw.pixels[..raw.get_size() * step].chunks_exact(step);
        let blt_data = &mut blt.0[..raw.get_size()];
        for (r, b) in raw_data.zip(blt_data.iter_mut()) {
            b.red = r[0];
            b.green = r[1];
            b.blue = r[2];
        }

        screen.draw_image(raw.header.width, raw.header.height, &blt.0)?;
    } else {
        // 从头读
        file.set_position(0)?;
    }
    
    Ok(())
}