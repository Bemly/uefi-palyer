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

use alloc::format;
use alloc::string::String;
use uefi::boot::{self, get_handle_for_protocol, open_protocol_exclusive};
use uefi::data_types::CString16;
use uefi::proto::console::text::{Input, Key};
use uefi::system::with_config_table;
use uefi::table::cfg::ConfigTableEntry as cfg;

use crate::error::{NyaStatus, Result};
use crate::fs::Fs;
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

/// 存报告到启动卷根目录，文件名自动递增（`{prefix}00.TXT`、`{prefix}01.TXT`…），
/// 连测多台机器不互相覆盖。返回屏显用的确认行。
pub fn save_report(prefix: &str, body: &[u8]) -> Result<String> {
    let mut fs = Fs::new()?;
    for i in 0..100u32 {
        let name = format!("{}{:02}.TXT", prefix, i);
        let cstr =
            CString16::try_from(name.as_str()).map_err(|_| NyaStatus::FromStrWithBufError)?;
        if fs.open_file(&cstr).is_err() {
            fs.write_file(&cstr, body)?;
            return Ok(format!("saved: {} ({} bytes)", name, body.len()));
        }
    }
    Err(NyaStatus::_Debug(String::from("too many reports on disk")))
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

/// CPUID 最大基本功能号（EAX=0）。读 7 号叶前必须先门控：
/// 老 CPU 没有 7 号叶，直接读会回落返回最大叶内容造成误报。
#[cfg(target_arch = "x86_64")]
pub fn cpuid_max_basic() -> u32 {
    core::arch::x86_64::__cpuid(0).eax
}

/// CPUID 最大扩展功能号（EAX=0x80000000）。读 0x80000001 叶前先门控，同上。
#[cfg(target_arch = "x86_64")]
pub fn cpuid_max_extended() -> u32 {
    core::arch::x86_64::__cpuid(0x8000_0000).eax
}
