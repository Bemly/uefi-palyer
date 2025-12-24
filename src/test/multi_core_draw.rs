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
struct DrawContext {
    fb_base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    num_cores: usize,
    mp: *const MpServices,
    frame_data: *const BltPixel, // 视频帧数据基地址
}

#[inline(always)]
unsafe fn u64_fast_copy(src: *const u8, dst: *mut u8, len: usize) {
    let mut i = 0;
    while i + 64 <= len {
        let s0 = (src.add(i) as *const u64).read_unaligned();
        let s1 = (src.add(i + 8) as *const u64).read_unaligned();
        let s2 = (src.add(i + 16) as *const u64).read_unaligned();
        let s3 = (src.add(i + 24) as *const u64).read_unaligned();
        let s4 = (src.add(i + 32) as *const u64).read_unaligned();
        let s5 = (src.add(i + 40) as *const u64).read_unaligned();
        let s6 = (src.add(i + 48) as *const u64).read_unaligned();
        let s7 = (src.add(i + 56) as *const u64).read_unaligned();

        (dst.add(i) as *mut u64).write_volatile(s0);
        (dst.add(i + 8) as *mut u64).write_volatile(s1);
        (dst.add(i + 16) as *mut u64).write_volatile(s2);
        (dst.add(i + 24) as *mut u64).write_volatile(s3);
        (dst.add(i + 32) as *mut u64).write_volatile(s4);
        (dst.add(i + 40) as *mut u64).write_volatile(s5);
        (dst.add(i + 48) as *mut u64).write_volatile(s6);
        (dst.add(i + 56) as *mut u64).write_volatile(s7);
        i += 64;
    }
    while i + 8 <= len {
        let val = (src.add(i) as *const u64).read_unaligned();
        (dst.add(i) as *mut u64).write_volatile(val);
        i += 8;
    }
    while i < len {
        dst.add(i).write_volatile(src.add(i).read());
        i += 1;
    }
}

extern "efiapi" fn ap_draw_task(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &*(arg as *const DrawContext) };
    let mp = unsafe { &*ctx.mp };

    let proc_num = mp.who_am_i().unwrap_or(0);
    
    // 计算分块区域 (横向切分)
    let block_height = ctx.height / ctx.num_cores;
    let start_y = proc_num * block_height;
    let end_y = if proc_num == ctx.num_cores - 1 {
        ctx.height
    } else {
        (proc_num + 1) * block_height
    };

    let row_bytes = ctx.width * 4;
    let stride_bytes = ctx.stride * 4;
    let src_base = ctx.frame_data as *const u8;

    loop {
        for y in start_y..end_y {
            unsafe {
                let row_src = src_base.add(y * row_bytes);
                let row_dest = ctx.fb_base.add(y * stride_bytes);
                u64_fast_copy(row_src, row_dest, row_bytes);
            }
        }
    }
}

pub fn multi_core_draw() {
    info!("=== Starting Multi-Processor (MP) Video Copy Test ===");
    
    // 1. 获取 MP 服务
    let mp_handle = get_handle_for_protocol::<MpServices>().unwrap();
    let mp = open_protocol_exclusive::<MpServices>(mp_handle).unwrap();
    let num_proc = mp.get_number_of_processors().unwrap();

    // 2. 获取 GOP 服务
    let gop_handle = get_handle_for_protocol::<GraphicsOutput>().unwrap();
    let mut gop = open_protocol_exclusive::<GraphicsOutput>(gop_handle).unwrap();
    
    let mode_info = gop.current_mode_info();
    let (width, height) = mode_info.resolution();
    let stride = mode_info.stride();
    let mut fb = gop.frame_buffer();
    let fb_base = fb.as_mut_ptr();

    // 3. 加载视频数据 (这里假设你要测试读取内存输出，需要提供一个数据源)
    // 如果没有真实视频数据，我们这里分配一个临时内存模拟
    let pixel_count = width * height;
    let mut fake_video_frame = alloc::vec![BltPixel::new(0, 0, 0); pixel_count];
    
    // 为测试填充点颜色
    for (i, p) in fake_video_frame.iter_mut().enumerate() {
        let x = i % width;
        let y = i / width;
        p.red = (x % 256) as u8;
        p.green = (y % 256) as u8;
        p.blue = ((x + y) % 256) as u8;
    }

    let ctx = DrawContext {
        fb_base,
        width,
        height,
        stride,
        num_cores: num_proc.enabled,
        mp: &*mp as *const MpServices,
        frame_data: fake_video_frame.as_ptr(),
    };

    let arg_ptr = &ctx as *const _ as *mut c_void;

    info!("Dispatching video copy task to all APs...");
    
    if num_proc.enabled > 1 {
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

    // BSP 也参与
    ap_draw_task(arg_ptr);
}


