#![no_main]
#![no_std]

use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, BltPixel, BltOp};
use uefi::proto::console::pointer::Pointer;
use core::time::Duration;

#[entry]
fn main() -> Status {
    let gop_handle = boot::get_handle_for_protocol::<GraphicsOutput>().expect("找不到 GOP");
    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle).expect("无法打开 GOP");

    let ptr_handle = boot::get_handle_for_protocol::<Pointer>().expect("找不到鼠标设备");
    let mut ptr = boot::open_protocol_exclusive::<Pointer>(ptr_handle).expect("无法打开鼠标协议");
    
    ptr.reset(false).expect("无法重置鼠标");

    // 屏幕底色初始化（黑色）
    gop.blt(BltOp::VideoFill {
        color: BltPixel::new(0, 0, 0),
        dest: (0, 0),
        dims: (800, 600),
    }).unwrap();

    // 绘制两个按钮
    draw_button(&mut gop, (100, 100), (200, 50), BltPixel::new(128, 128, 128));
    draw_button(&mut gop, (100, 200), (200, 50), BltPixel::new(128, 128, 128));

    let mut cursor_x = 400i32; // 从屏幕中间开始
    let mut cursor_y = 300i32;
    let mut old_x = cursor_x;
    let mut old_y = cursor_y;

    loop {
        if let Ok(Some(state)) = ptr.read_state() {
            // 1. 擦除旧光标 (用背景色黑色填充)
            // 注意：如果光标经过按钮，这里会把按钮擦掉一部分。
            // 高级做法是保存背景像素，这里为了简单先用黑色填充。
            draw_cursor(&mut gop, (old_x as usize, old_y as usize), BltPixel::new(0, 0, 0));

            // 2. 更新坐标
            cursor_x += state.relative_movement[0] / 2; // 除以2减慢速度
            cursor_y += state.relative_movement[1] / 2;
            cursor_x = cursor_x.clamp(0, 790);
            cursor_y = cursor_y.clamp(0, 590);

            // 3. 检查按钮点击
            if state.button[0] {
                if cursor_x >= 100 && cursor_x <= 300 && cursor_y >= 100 && cursor_y <= 150 {
                     draw_button(&mut gop, (100, 100), (200, 50), BltPixel::new(0, 255, 0));
                }
                if cursor_x >= 100 && cursor_x <= 300 && cursor_y >= 200 && cursor_y <= 250 {
                     draw_button(&mut gop, (100, 200), (200, 50), BltPixel::new(0, 0, 255));
                }
            }

            // 4. 绘制新光标 (白色小方块)
            draw_cursor(&mut gop, (cursor_x as usize, cursor_y as usize), BltPixel::new(255, 255, 255));

            // 更新旧坐标
            old_x = cursor_x;
            old_y = cursor_y;
        }

        boot::stall(Duration::from_millis(16)); // 约 60 FPS
    }
}

fn draw_button(gop: &mut GraphicsOutput, dest: (usize, usize), dims: (usize, usize), color: BltPixel) {
    gop.blt(BltOp::VideoFill { color, dest, dims }).expect("填充失败");
}

// 绘制一个 10x10 的光标
fn draw_cursor(gop: &mut GraphicsOutput, dest: (usize, usize), color: BltPixel) {
    gop.blt(BltOp::VideoFill {
        color,
        dest,
        dims: (10, 10),
    }).ok();
}