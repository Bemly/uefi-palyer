use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;
use uefi::boot::{find_handles, get_handle_for_protocol, open_protocol_exclusive, ScopedProtocol};
use uefi::{boot, println};
use uefi::proto::console::gop::{BltOp, BltPixel, GraphicsOutput, ModeInfo};
use crate::video::ascii_font::FONT_8X16;

fn info2str(a: &str, info: ModeInfo) -> String {
    let r = format!("resolution:{:?}", info.resolution());
    let f = format!("pixel_format:{:?}", info.pixel_format());
    let s = format!("stride:{:?}", info.stride());
    let b = format!("pixel_bitmask:{:?}", info.pixel_bitmask());
    format!("[{}]{} {} {} {}", a, r, f, s, b)
}

static mut OFFSET: usize = 0;
fn println(gop: &mut ScopedProtocol<GraphicsOutput>, text: &str) {
    let mut x = 0;
    // 获取当前屏幕的宽度，用于自动换行
    let (width, height) = gop.current_mode_info().resolution();

    let fg = BltPixel::new(255, 255, 255);
    let bg = BltPixel::new(0, 0, 0);

    // 超出屏幕高度自动归零
    unsafe { if OFFSET + 18 >= height { OFFSET = 0 } }

    for c in text.chars() {
        if c == '\n' {
            x = 0;
            unsafe { OFFSET += 16; }
            continue;
        }

        // 超出屏幕宽度自动换行 (8 是字符宽度)
        if x + 8 > width {
            x = 0;
            unsafe { OFFSET += 16; }
        }

        // --- 2. 绘制字符 ---

        let index = (c as usize) & 0x7F;
        let glyph = &FONT_8X16[index];

        for row in 0..16 {
            let row_bits = glyph[row];
            for col in 0..8 {
                let is_fg = (row_bits >> (7 - col)) & 1 == 1;
                let color = if is_fg { fg } else { bg };

                // 绘制像素
                let _ = gop.blt(BltOp::VideoFill {
                    color,
                    dest: (x + col, unsafe { OFFSET + row }),
                    dims: (1, 1),
                });
            }
        }

        // 字符绘制完后，坐标右移
        x += 8;
    }

    // 整个字符串画完后，最后额外换一行，防止下一次打印重叠
    unsafe { OFFSET += 18 }
}

pub fn get_multi_mode() {
    let gop = get_handle_for_protocol::<GraphicsOutput>().unwrap();
    let mut gop = open_protocol_exclusive::<GraphicsOutput>(gop).unwrap();

    // 1. 获取所有模式
    // 先查后改
    let mode_strings: Vec<String> = gop.modes()
        .map(|mode| info2str("query mode", *mode.info()))
        .collect();

    for s in mode_strings {
        println(&mut gop, &s);
    }

    // 2. 获取当前模式
    let info = gop.current_mode_info();
    println(&mut gop, &info2str("current mode", info))
}

// 经过测试发现大部分GOP驱动都只支持给我一个Handle(也就是一个显示器)
// 那多显示器已经毫无意义，已经回滚到单显示器
pub fn get_multi_monitor() {
    let handle = find_handles::<GraphicsOutput>().expect("Failed to find handles");

    // 可以用ok().map() 但是可读性不是很好
    let info: Vec<Vec<String>> = handle.iter()
        .filter_map(|&h| {
            if let Ok(gop) = open_protocol_exclusive::<GraphicsOutput>(h) {
                // 获取所有模式
                let mut mode_strings: Vec<String> = gop.modes()
                    .map(|mode| {
                        let s = &format!("query {:?}", h);
                        info2str(s, *mode.info())
                    })
                    .collect();
                // 获取当前模式
                let s = format!("current {:?}", h);
                let s = info2str(&s, gop.current_mode_info());
                mode_strings.push(s);
                Some(mode_strings)
            } else { None }
        }).collect();

    if let Some(h) = handle.first() {
        let mut gop = open_protocol_exclusive::<GraphicsOutput>(*h).unwrap();
        for mode_strings in info {
            for s in mode_strings {
                println(&mut gop, &s);
            }
        }
    }
    // for h in handle {
    //     let mut gop = open_protocol_exclusive::<GraphicsOutput>(h).expect("Failed to open protocol");
    //
    //     let mode_strings: Vec<String> = gop.modes()
    //         .map(|mode| info2str("query mode", *mode.info()))
    //         .collect();
    //     for s in mode_strings {
    //         println(&mut gop, &s);
    //     }
    // }
}

pub fn test_reopen_protocol() { // it work! : )
    {
        let gop = get_handle_for_protocol::<GraphicsOutput>().unwrap();
        let gop = open_protocol_exclusive::<GraphicsOutput>(gop).unwrap();
        let _ = gop.current_mode_info().resolution();
    }

    {
        let gop = get_handle_for_protocol::<GraphicsOutput>().unwrap();
        let mut gop = open_protocol_exclusive::<GraphicsOutput>(gop).unwrap();
        println(&mut gop, "reopen protocol");
    }

    {
        let gop = get_handle_for_protocol::<GraphicsOutput>().unwrap();
        let mut gop = open_protocol_exclusive::<GraphicsOutput>(gop).unwrap();
        println(&mut gop, "reopen protocol again");
    }
}

pub fn test_reopen_multi_protocol() { // it dont work :(
    {
        let handle = find_handles::<GraphicsOutput>().unwrap();
        for h in handle {
            let _ = open_protocol_exclusive::<GraphicsOutput>(h);
            // qemu第二个gop被第一个gop占用了，所以这里会失败,故不要unwarp
        }
    }

    {
        let handle = find_handles::<GraphicsOutput>().unwrap();
        if let Some(h) = handle.first() {
            let mut gop = open_protocol_exclusive::<GraphicsOutput>(*h).unwrap();
            println(&mut gop, "reopen protocol");
        }
    }
}
