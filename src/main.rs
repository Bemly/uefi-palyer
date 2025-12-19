#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use uefi::prelude::*;
use core::time::Duration;
use log::info;
use uefi::boot::{get_handle_for_protocol, get_image_file_system, image_handle, open_protocol_exclusive, stall};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode};

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    let width = 19200;
    let height = 10800;
    let pixel_count = width * height;
    let bytes_needed = pixel_count * core::mem::size_of::<BltPixel>();

    info!("正在申请内存: {} 像素, 约 {} KB", pixel_count, bytes_needed / 1024);

    let mut blt_buf = vec![BltPixel::new(0,0,0); pixel_count];

    info!("succs"); // 如果闪退，这行就不会打印


    stall(Duration::from_mins(1));
    Status::SUCCESS
}