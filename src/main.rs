#![no_main]
#![no_std]

extern crate alloc;

mod fs;
mod graphics;
mod error;

use alloc::format;
use alloc::vec::Vec;
use uefi::prelude::*;
use uefi::proto::console::gop::BltPixel;
use core::time::Duration;
use log::info;
use uefi::CStr16;
use crate::error::{handle_fatal, NyaStatus, Result};
use crate::fs::Fs;
use crate::graphics::Screen;

#[entry]
fn main() -> Status {
    run().unwrap_or_else(|e| handle_fatal(e));
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}

fn run() -> Result {
    uefi::helpers::init()?;
    info!("UEFI Booting...");

    let mut fs = Fs::new()?;
    let mut path_buf = [0u16; 128];
    let mut screen = Screen::new()?;
    loop {
        for i in 1..98 {
            let path = format!("anime\\{:04}.qoi", i);
            let path = CStr16::from_str_with_buf(&path, &mut path_buf)
                .map_err(|_| NyaStatus::FromStrWithBufError)?;
            draw(&mut fs, &mut screen, path)?;
            boot::stall(Duration::from_millis(1000 / 90));
        }
    }


    Ok(())
}

fn draw(fs: &mut Fs, screen: &mut Screen, path: &CStr16) -> Result {
    // 1. 加载文件
    let qoi_data = fs.read_file(path)?;

    // 2. 解码图像
    let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)?;

    // 3. 转换像素格式 (Iterator 适配器模式)
    let blt_buf: Vec<BltPixel> = rgba_buf
        .chunks_exact(header.channels.as_u8() as usize)
        .map(|c| BltPixel::new(c[0], c[1], c[2]))
        .collect();

    // 4. 渲染
    screen.draw_image(header.width, header.height, &blt_buf)?;

    Ok(())
}