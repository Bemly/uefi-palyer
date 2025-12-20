#![no_main]
#![no_std]

extern crate alloc;

mod fs;
mod graphics;

use alloc::vec::Vec;
use uefi::prelude::*;
use uefi::proto::console::gop::BltPixel;
use core::time::Duration;
use log::info;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("UEFI Booting...");

    // 1. 加载文件
    let qoi_data = fs::read_file(cstr16!("test2.qoi"));

    // 2. 解码图像
    let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)
        .expect("Decode failed");

    // 3. 转换像素格式 (Iterator 适配器模式)
    let blt_buf: Vec<BltPixel> = rgba_buf
        .chunks_exact(header.channels.as_u8() as usize)
        .map(|c| BltPixel::new(c[0], c[1], c[2]))
        .collect();

    // 4. 渲染
    let mut screen = graphics::Screen::new();
    screen.draw_image(header.width, header.height, &blt_buf);

    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}