#![no_main]
#![no_std]

use uefi::prelude::*;
use core::time::Duration;
use log::info;
use uefi::boot::{get_image_file_system, image_handle, stall};
use uefi::proto::media::file::{File, FileAttribute, FileMode};

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    info!("Hello world!");

    let mut root_dir = get_image_file_system(image_handle())
        .expect("Failed to get FAT protocol")
        .open_volume()
        .expect("Failed to open EFI partition");

    let mut file =root_dir
        .open(cstr16!("hello.txt"), FileMode::Read, FileAttribute::empty())
        .expect("Failed to open hello.txt")
        .into_regular_file()
        .expect("hello.txt is not a regular file");

    info!("Opened hello.txt {}", file.get_position().expect("Failed to get file position"));

    let mut buf = [0u8; 128];
    while let Ok(read_len) = file.read(&mut buf) {
        if read_len == 0 { break }

        let data = &buf[..read_len];

        if let Ok(text) = core::str::from_utf8(data) {
            info!("{}", text);
        }
    }

    info!("hello.txt output completed.");

    stall(Duration::from_mins(10));
    Status::SUCCESS
}