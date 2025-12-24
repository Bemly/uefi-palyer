use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use uefi::prelude::*;
use core::time::Duration;
use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive};
use uefi::println;
use uefi::proto::pi::mp::MpServices;
use log::{error, info, warn};
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat, BltPixel};

#[repr(C)]
struct MpDrawContext<'a> {
    mp: &'a MpServices,
    fb_base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    num_total_procs: usize,
}

#[inline(always)]
unsafe fn u64_fast_copy_color(dst: *mut u8, len: usize, color_u64: u64) {
    let mut i = 0;
    // 每次填充 64 字节 (8个 u64)
    while i + 64 <= len {
        let ptr = dst.add(i) as *mut u64;
        ptr.write_volatile(color_u64);
        ptr.add(1).write_volatile(color_u64);
        ptr.add(2).write_volatile(color_u64);
        ptr.add(3).write_volatile(color_u64);
        ptr.add(4).write_volatile(color_u64);
        ptr.add(5).write_volatile(color_u64);
        ptr.add(6).write_volatile(color_u64);
        ptr.add(7).write_volatile(color_u64);
        i += 64;
    }
    while i + 8 <= len {
        (dst.add(i) as *mut u64).write_volatile(color_u64);
        i += 8;
    }
    while i < len {
        dst.add(i).write_volatile(color_u64 as u8);
        i += 1;
    }
}

extern "efiapi" fn ap_draw_task(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &*(arg as *const MpDrawContext) };

    // 获取当前核心索引
    let proc_num = ctx.mp.who_am_i().unwrap_or(0);

    // --- 核心逻辑：划分连续区域 ---
    // 每个核心负责的行数 = 总行数 / 总核心数
    let rows_per_core = ctx.height / ctx.num_total_procs;
    let start_row = proc_num * rows_per_core;

    // 最后一个核心负责处理剩余的所有行（防止除不尽）
    let end_row = if proc_num == ctx.num_total_procs - 1 {
        ctx.height
    } else {
        start_row + rows_per_core
    };

    // 每个核心使用不同的颜色（示例：基于 proc_num 生成简单的颜色）
    // 0x00RRGGBB (UEFI 通常是 BGR0 或 RGB0)
    let color: u32 = match proc_num % 4 {
        0 => 0x00FF0000, // 红色
        1 => 0x0000FF00, // 绿色
        2 => 0x000000FF, // 蓝色
        _ => 0x00FFFF00, // 黄色
    };
    // 构造 64 位的颜色块以便 fast_copy
    let color_u64 = ((color as u64) << 32) | (color as u64);

    // 计算该核心负责的显存起始位置
    let bytes_per_pixel = 4;
    let row_size_in_bytes = ctx.stride * bytes_per_pixel;

    for y in start_row..end_row {
        unsafe {
            let row_ptr = ctx.fb_base.add(y * row_size_in_bytes);
            // 只填充这一行中可见的宽度部分
            u64_fast_copy_color(row_ptr, ctx.width * bytes_per_pixel, color_u64);
        }
    }
}

pub fn multi_core_draw() {
    info!("=== Starting Multi-Processor (MP) Video Copy Test ===");

    // 1. 获取 MP 服务
    let mp_handle = get_handle_for_protocol::<MpServices>().unwrap();
    let mp = open_protocol_exclusive::<MpServices>(mp_handle).unwrap();

    // 2. 获取 GOP 服务
    let gop_handle = get_handle_for_protocol::<GraphicsOutput>().unwrap();
    let mut gop = open_protocol_exclusive::<GraphicsOutput>(gop_handle).unwrap();

    let mode_info = gop.current_mode_info();
    let (width, height) = mode_info.resolution();
    let stride = mode_info.stride();
    let mut fb = gop.frame_buffer();
    let fb_base = fb.as_mut_ptr();

    let num_proc = mp.get_number_of_processors().unwrap();
    let total_cores = num_proc.enabled;

    let ctx = MpDrawContext {
        mp: &mp,
        fb_base,
        width,
        height,
        stride,
        num_total_procs: total_cores,
    };

    let arg_ptr = &ctx as *const _ as *mut c_void;

    if total_cores > 1 {
        // 启动除 BSP 以外的所有 AP
        // uefi-rs 的 startup_all_aps 在这里会阻塞直到 AP 运行结束
        if let Err(e) = mp.startup_all_aps(
            false,
            ap_draw_task,
            arg_ptr,
            None,
            None
        ) {
            error!("Failed to start APs: {:?}", e);
        }
    }

    // BSP 亲自处理它自己的那一份区域
    ap_draw_task(arg_ptr);

    info!("All cores finished drawing.");
}


