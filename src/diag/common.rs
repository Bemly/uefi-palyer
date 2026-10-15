//! diag 公共 helpers：双写输出、按键等待、SMBIOS 寻址。
//!
//! 背景：本项目历史上混用了 4 种屏显/日志手段
//! （`Screen::draw_str` / `uefi::println!` / `log::*` / embedded-graphics），
//! 各自成因不同。收敛策略：
//! - 图形模式下的用户可见输出统一走 `Screen::draw_str`（`handle_fatal` 用的就是它）；
//! - 串口日志保留（`log::*` 可被 QEMU `-serial` 捕获），所以提供 `emit!` 双写；
//! - `get_multi_monitor.rs` 里那个手写的本地 `println + static mut OFFSET`
//!   是 `Screen::draw_str` 的重复实现，已删除；
//! - 帧内 FPS/OSD（`video.rs::draw_string_opaque`）是显存直写 HUD，不属于文本输出，不动。

use core::time::Duration;

use uefi::boot::{self, get_handle_for_protocol, open_protocol_exclusive};
use uefi::proto::console::text::{Input, Key};
use uefi::system::with_config_table;
use uefi::table::cfg::ConfigTableEntry as cfg;

use crate::graphics::Screen;

/// 双写输出：串口日志（`log`）+ 图形屏显（`Screen::draw_str`），至少一处可见。
///
/// 用法：`emit!(screen, "Total cores: {}", n);`
#[macro_export]
macro_rules! emit {
    ($screen:expr, $($arg:tt)*) => {{
        log::info!($($arg)*);
        $screen.draw_str(&alloc::format!($($arg)*));
    }};
}

fn key_to_char(key: Key) -> Option<char> {
    match key {
        Key::Printable(c) => char::from_u32(u16::from(c) as u32),
        Key::Special(_) => None,
    }
}

/// 排空输入队列里的陈旧按键，避免菜单刚画出来就被之前的按键误触发。
pub fn flush_keys() {
    if let Ok(handle) = get_handle_for_protocol::<Input>() {
        if let Ok(mut input) = open_protocol_exclusive::<Input>(handle) {
            while matches!(input.read_key(), Ok(Some(_))) {}
        }
    }
}

/// 阻塞等待一个可打印按键，返回其小写形式。
pub fn wait_key() -> char {
    loop {
        if let Ok(handle) = get_handle_for_protocol::<Input>() {
            if let Ok(mut input) = open_protocol_exclusive::<Input>(handle) {
                // 先排空陈旧按键，再阻塞轮询。
                while matches!(input.read_key(), Ok(Some(_))) {}
                loop {
                    if let Ok(Some(key)) = input.read_key() {
                        if let Some(c) = key_to_char(key) {
                            return c.to_ascii_lowercase();
                        }
                        // 功能键（方向键/Fx/ESC 等）：继续等可打印字符。
                    }
                    boot::stall(Duration::from_millis(50));
                }
            }
        }
        boot::stall(Duration::from_millis(200));
    }
}

/// 子项跑完后的统一收尾：提示 + 等任意键回菜单。
pub fn pause_return(screen: &mut Screen) {
    screen.draw_str("-- Done. Press any key back to menu --");
    flush_keys();
    wait_key();
}

/// 在当前 config table 里找 SMBIOS 入口物理地址（优先 SMBIOS3，回落 SMBIOS）。
///
/// 原来 `get_smbios.rs` 两个函数各抄了一份 10 行查找逻辑，抽到这里。
/// 返回 `usize` 形式的物理地址，调用方按需转指针。
pub fn find_smbios_address() -> Option<usize> {
    let mut found: Option<usize> = None;
    with_config_table(|tables| {
        for entry in tables {
            match entry.guid {
                cfg::SMBIOS3_GUID => {
                    found = Some(entry.address as usize);
                    break;
                }
                cfg::SMBIOS_GUID => {
                    found = Some(entry.address as usize);
                }
                _ => {}
            }
        }
    });
    found
}
