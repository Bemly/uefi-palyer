#![no_main]
#![no_std]

extern crate alloc;

mod diag;
mod error;
mod fs;
mod graphics;
mod menu;
mod video;

use uefi::prelude::*;

use crate::graphics::Screen;
use crate::menu::run_menu;

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    let screen = Screen::new().expect("Failed to create screen");
    // 默认停留等待：由菜单分发播放视频或各诊断项，不再直进 video_run。
    run_menu(screen);
}
