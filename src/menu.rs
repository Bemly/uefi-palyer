//! 开机菜单：默认停留等待按键，按 0-7 进对应项，r 重启。
//!
//! 错误策略：
//! - 播放视频（生产路径）失败 → [`handle_fatal`]，和重构前 `main` 语义完全一致；
//! - 诊断项失败 → 可返回式报错显示，按任意键回菜单；
//! - [`handle_fatal`] 本体（`error.rs`）一字不动。

use core::time::Duration;

use uefi::boot::stall;

use crate::diag::common::{flush_keys, pause_return, wait_key};
use crate::diag::registry::{Entry, ITEMS};
use crate::error::{handle_fatal, NyaStatus, Result};
use crate::graphics::Screen;

fn draw_menu(screen: &mut Screen) {
    let _ = screen.clear();
    screen.reset_cursor();
    screen.draw_str("== UEFI Diag Menu ==");
    for item in ITEMS {
        screen.draw_str(&alloc::format!("[{}] {} - {}", item.key, item.name, item.desc));
    }
    screen.draw_str("[r] reboot");
    screen.draw_str("Press key to select:");
}

/// 诊断项的可返回式报错：显示错误，等任意键回菜单（不 halt）。
fn show_recoverable(screen: &mut Screen, err: NyaStatus) {
    let _ = screen.clear();
    screen.reset_cursor();
    screen.draw_str(&alloc::format!("ERROR: {:?}", err));
    pause_return(screen);
}

fn run_shared(screen: &mut Screen, f: fn(&mut Screen) -> Result, fatal: bool) {
    let _ = screen.clear();
    screen.reset_cursor();
    match f(screen) {
        Ok(()) => pause_return(screen),
        // 生产路径保持原语义：致命屏显（error.rs 原样保留）。
        Err(e) if fatal => handle_fatal(e, screen),
        Err(e) => show_recoverable(screen, e),
    }
}

pub fn run_menu(mut screen: Screen) -> ! {
    loop {
        draw_menu(&mut screen);
        flush_keys();
        let key = wait_key();

        if key == 'r' {
            uefi::runtime::reset(uefi::runtime::ResetType::COLD, uefi::Status::SUCCESS, None);
        }

        let mut matched = false;
        for item in ITEMS {
            if item.key == key {
                matched = true;
                match item.run {
                    Entry::Shared(f) => run_shared(&mut screen, f, item.fatal_on_error),
                    Entry::Exclusive(f) => {
                        // 该 demo 需要自己独占 GOP（含切显示模式），
                        // 先释放 Screen 再跑，跑完重建 Screen 回菜单。
                        // 注意：这里不清屏——demo 画面要留给用户看，
                        // 提示行打在底部。
                        drop(screen);
                        let result = f();
                        screen =
                            Screen::reacquire().expect("Failed to re-create screen");
                        screen.cursor_to_bottom();
                        match result {
                            Ok(()) => pause_return(&mut screen),
                            Err(e) => show_recoverable(&mut screen, e),
                        }
                    }
                }
                break;
            }
        }

        if !matched {
            screen.draw_str("Unknown key, try again.");
            stall(Duration::from_millis(500));
        }
    }
}
