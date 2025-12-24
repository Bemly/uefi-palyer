use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::graphics::Screen;

pub fn get_multi_monitor() {
    let mut screen = Screen::new().expect("Failed to create screen");
    screen.clear().expect("Failed to clear screen");


    let mode_strings: Vec<String> = screen.get_gop().modes()
        .map(|mode| {
            let info =  mode.info();
            let r = format!("resolution:{:?}", info.resolution());
            let f = format!("pixel_format:{:?}", info.pixel_format());
            let s = format!("stride:{:?}", info.stride());
            let b = format!("pixel_bitmask:{:?}", info.pixel_bitmask());
            format!("{} {} {} {}", r, f, s, b)
        })
        .collect();

    for s in mode_strings {
        screen.draw_str(&s);
    }
}