#![no_main]
#![no_std]

extern crate alloc;

mod fs;
mod graphics;
mod error;
mod video;
mod test;

use uefi::prelude::*;
use core::time::Duration;
use crate::error::handle_fatal;
use crate::graphics::Screen;
use crate::test::get_cpu_info::get_cpu_info;
use crate::test::use_embedded_graphics_gop::use_embedded_graphics_gop;
use crate::video::video_run;

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    // let mut screen = Screen::new().expect("Failed to create screen");
    // video_run(&mut screen).unwrap_or_else(|e| handle_fatal(e, &mut screen));
    // use_embedded_graphics_gop();
    get_cpu_info();

    boot::stall(Duration::from_mins(2));
    Status::SUCCESS
}
