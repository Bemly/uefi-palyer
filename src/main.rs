#![no_main]
#![no_std]

extern crate alloc;

mod fs;
mod graphics;
mod error;

use alloc::vec::Vec;
use uefi::prelude::*;
use uefi::proto::console::gop::BltPixel;
use core::time::Duration;
use log::info;
use crate::error::{handle_fatal, Result};

#[entry]
fn main() -> Status {
    run().unwrap_or_else(|e| handle_fatal(e));
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}

fn run() -> Result {
    uefi::helpers::init()?;
    info!("UEFI Booting...");

    // 1. 加载文件
    let qoi_data = fs::read_file(cstr16!("test2.qoi"))?;

    // 2. 解码图像
    let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)?;

    // 3. 转换像素格式 (Iterator 适配器模式)
    let blt_buf: Vec<BltPixel> = rgba_buf
        .chunks_exact(header.channels.as_u8() as usize)
        .map(|c| BltPixel::new(c[0], c[1], c[2]))
        .collect();

    // 4. 渲染
    let mut screen = graphics::Screen::new()?;
    screen.draw_image(header.width, header.height, &blt_buf)?;

    Ok(())
}