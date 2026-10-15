//! GOP 压测套件：显示管线的单核/多核/带宽/CPU/显卡瓶颈归因。
//!
//! 由 `hw_bench` 在 TSC 校准后调用，结果并入同一份报告。分三层：
//! - 基线（搬迁自 hw_bench）：整屏 VideoFill、BufferToVideo、framebuffer 直写；
//! - 定标：1/4 面积 BLT/Fill，对比整屏，分离引擎固定开销与像素速率；
//! - 瓶颈：单核 vs 多核显存填充（扩展比）、CPU 端像素换色吞吐、
//!   最后按数字给瓶颈归因（传输/引擎/CPU/并行效率）。
//!
//! x86_64 only（RDTSC 计时 + MP 多核），由调用方保证。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

use uefi::boot::{
    create_event, get_handle_for_protocol, open_protocol_exclusive, EventType, Tpl,
};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion};
use uefi::proto::pi::mp::MpServices;

use crate::error::Result;
use crate::graphics::Screen;

macro_rules! tline {
    ($report:expr, $screen:expr, $($arg:tt)*) => {{
        let s = alloc::format!($($arg)*);
        log::info!("{}", s);
        $screen.draw_str(&s);
        $report.push(s);
    }};
}

#[inline(always)]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

pub fn run_section(report: &mut Vec<String>, screen: &mut Screen, tps: u64) -> Result {
    let (w, h) = screen.get_gop().current_mode_info().resolution();
    let stride = screen.get_gop().current_mode_info().stride();
    tline!(report, screen, "-- gop --");
    tline!(report, screen, "gop: {}x{} stride {}", w, h, stride);

    let fb_bytes = w * h * 4;

    // ---- 基线 1：整屏 VideoFill（引擎填充，不搬数据） ----
    const FILL_N: u64 = 10;
    let t0 = rdtsc();
    for _ in 0..FILL_N {
        screen.clear()?;
    }
    let dt = rdtsc() - t0;
    let fill_fps = (FILL_N as f64 * tps as f64) / dt as f64;
    tline!(
        report,
        screen,
        "gop VideoFill full: {:.1} fps ({:.2} ms/frame)",
        fill_fps,
        dt as f64 * 1000.0 / (FILL_N as f64 * tps as f64)
    );

    // ---- 基线 2：整屏 BufferToVideo（CPU 内存 -> 显存搬运） ----
    let pixels = vec![BltPixel::new(0x11, 0x22, 0x33); w * h];
    const BLT_N: u64 = 10;
    let t0 = rdtsc();
    for _ in 0..BLT_N {
        screen.draw_image(w as u32, h as u32, &pixels)?;
    }
    let dt = rdtsc() - t0;
    let blt_fps = (BLT_N as f64 * tps as f64) / dt as f64;
    let blt_mbs = (BLT_N * fb_bytes as u64) as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0);
    tline!(
        report,
        screen,
        "gop BufferToVideo full: {:.1} fps, {:.1} MB/s",
        blt_fps,
        blt_mbs
    );

    // ---- 基线 3：framebuffer 裸写（BSP 直写，无 BLT 引擎参与） ----
    const FB_N: u64 = 10;
    let t0 = rdtsc();
    {
        let mut fb = screen.get_gop().frame_buffer();
        let dst = fb.as_mut_ptr();
        for _ in 0..FB_N {
            unsafe {
                let mut i = 0;
                while i + 8 <= fb_bytes {
                    (dst.add(i) as *mut u64).write_volatile(0x0011_2233_0011_2233);
                    i += 8;
                }
            }
        }
    }
    let dt = rdtsc() - t0;
    let fb_mbs = (FB_N * fb_bytes as u64) as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0);
    tline!(report, screen, "gop direct-write full: {:.1} MB/s", fb_mbs);

    // ---- 定标：1/4 面积，分离固定开销与像素速率 ----
    // 设 T = F + k*P，由整屏(P)与1/4屏(P/4)解出 F（单次调用固定开销）。
    let qw = w / 2;
    let qh = h / 2;
    let q_pixels = vec![BltPixel::new(0x11, 0x22, 0x33); qw * qh];
    let (dx, dy) = ((w - qw) / 2, (h - qh) / 2);
    const Q_N: u64 = 20;
    let t0 = rdtsc();
    for _ in 0..Q_N {
        screen.get_gop().blt(BltOp::BufferToVideo {
            buffer: &q_pixels,
            src: BltRegion::Full,
            dest: (dx, dy),
            dims: (qw, qh),
        })?;
    }
    let dt_q = rdtsc() - t0;
    let t_full_ms = dt as f64 * 1000.0 / (BLT_N as f64 * tps as f64);
    let t_q_ms = dt_q as f64 * 1000.0 / (Q_N as f64 * tps as f64);
    // F = (4*Tq - Tf) / 3
    let fixed_ms = (4.0 * t_q_ms - t_full_ms) / 3.0;
    tline!(
        report,
        screen,
        "gop BufferToVideo qtr: {:.2} ms (full {:.2} ms, fixed ~{:.2} ms)",
        t_q_ms,
        t_full_ms,
        if fixed_ms > 0.0 { fixed_ms } else { 0.0 }
    );

    let t0 = rdtsc();
    for _ in 0..Q_N {
        screen.get_gop().blt(BltOp::VideoFill {
            color: BltPixel::new(0, 0, 0),
            dest: (dx, dy),
            dims: (qw, qh),
        })?;
    }
    let dt_qf = rdtsc() - t0;
    tline!(
        report,
        screen,
        "gop VideoFill qtr: {:.2} ms",
        dt_qf as f64 * 1000.0 / (Q_N as f64 * tps as f64)
    );
    drop(q_pixels);
    drop(pixels);

    // ---- CPU 瓶颈：内存像素换色（播放管线每帧在 CPU 上干的活：RGBA->BGRA） ----
    const CONV_N: u64 = 5;
    let mut conv = vec![0x33u8; fb_bytes];
    // 先污染成非均匀内容，防编译器把循环优化成 memset。
    for (i, b) in conv.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }
    let t0 = rdtsc();
    for _ in 0..CONV_N {
        let buf = core::hint::black_box(conv.as_mut_ptr());
        unsafe {
            for i in (0..fb_bytes).step_by(4) {
                let r = buf.add(i).read();
                let bl = buf.add(i + 2).read();
                buf.add(i).write(bl);
                buf.add(i + 2).write(r);
            }
        }
    }
    let dt = rdtsc() - t0;
    core::hint::black_box(conv.as_ptr());
    let conv_mbs = (CONV_N * fb_bytes as u64) as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0);
    tline!(
        report,
        screen,
        "cpu convert RB-swap: {:.1} MB/s",
        conv_mbs
    );
    drop(conv);

    // ---- 单核 vs 多核：显存分区填充 ----
    let n_cores = match get_handle_for_protocol::<MpServices>() {
        Ok(h) => match open_protocol_exclusive::<MpServices>(h) {
            Ok(mp) => mp.get_number_of_processors().map(|c| c.enabled).unwrap_or(1),
            Err(_) => 1,
        },
        Err(_) => 1,
    };
    tline!(report, screen, "mp: {} enabled core(s)", n_cores);

    // 单核：BSP 自己填整屏，5 轮平均（与多核计时轮对称，消单发噪声）。
    const SINGLE_ROUNDS: u64 = 5;
    let t0 = rdtsc();
    for _ in 0..SINGLE_ROUNDS {
        let mut fb = screen.get_gop().frame_buffer();
        let dst = fb.as_mut_ptr();
        let stride_bytes = stride * 4;
        unsafe { fill_rows(dst, stride_bytes, w, 0, h, 0x0011_2233_0011_2233) };
    }
    let dt_single = rdtsc() - t0;
    let single_ms = dt_single as f64 * 1000.0 / (SINGLE_ROUNDS as f64 * tps as f64);
    tline!(
        report,
        screen,
        "fb fill 1 core: {:.2} ms ({:.1} MB/s)",
        single_ms,
        (SINGLE_ROUNDS * fb_bytes as u64) as f64 * tps as f64
            / (dt_single as f64 * 1024.0 * 1024.0)
    );

    // 多核：AP 常驻工作循环，BSP 发轮次号并行填。
    // 注意：单发 startup 测到的是 AP 唤醒开销（SIPI，十几 ms），不是扩展性——
    // 实测单核填整屏才 0.66ms，唤醒就把并行收益全淹了。所以这里 AP 起一次
    // 常驻循环，先热身 1 轮吸收唤醒延迟，再计时多轮取平均，测的是稳态吞吐。
    // 每轮 5 秒超时熔断，超时按部分结果报告，不炸整个测试。
    if n_cores > 1 {
        let mp_handle = get_handle_for_protocol::<MpServices>()?;
        let mp = open_protocol_exclusive::<MpServices>(mp_handle)?;
        let (fw, fh) = screen.get_gop().current_mode_info().resolution();
        let fst = screen.get_gop().current_mode_info().stride();
        let mut fb = screen.get_gop().frame_buffer();
        let fb_base = fb.as_mut_ptr();
        // 取裸指针后结束 GOP 借用：AP 与 BSP 只走裸指针，不再经过协议。
        let round = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let ctx = FillCtx {
            mp: &mp,
            fb_base,
            stride_bytes: fst * 4,
            width: fw,
            height: fh,
            n: n_cores,
            round: &round,
            done: &done,
        };
        let arg_ptr = &ctx as *const _ as *mut c_void;
        let event = unsafe { create_event(EventType::empty(), Tpl::CALLBACK, None, None)? };
        let my_id = mp.who_am_i().unwrap_or(0) % n_cores;
        let (by0, by1) = rows_for(my_id, n_cores, fh);
        let stride_b = fst * 4;
        let expected_aps = n_cores - 1;

        // 非阻塞启动：AP 进常驻循环，BSP 不等。
        mp.startup_all_aps(false, ap_fill_loop, arg_ptr, Some(event), None)
            .map_err(|e| e.status())?;

        // 热身轮（round 1，不计时）：吸收 AP 唤醒延迟。
        round.store(1, Ordering::SeqCst);
        unsafe { fill_rows(fb_base, stride_b, fw, by0, by1, 0x0044_5566_0044_5566) };
        let deadline = rdtsc() + 5 * tps.max(1);
        while done.load(Ordering::Acquire) < expected_aps && rdtsc() < deadline {
            core::hint::spin_loop();
        }

        // 计时轮。
        const TIMED_ROUNDS: u64 = 5;
        let mut degraded = done.load(Ordering::Acquire) < expected_aps;
        let t0 = rdtsc();
        for r in 2..2 + TIMED_ROUNDS as usize {
            round.store(r, Ordering::SeqCst);
            unsafe { fill_rows(fb_base, stride_b, fw, by0, by1, 0x0044_5566_0044_5566) };
            let target = r * expected_aps;
            let deadline = rdtsc() + 5 * tps.max(1);
            while done.load(Ordering::Acquire) < target && rdtsc() < deadline {
                core::hint::spin_loop();
            }
            if done.load(Ordering::Acquire) < target {
                degraded = true;
                break;
            }
        }
        let dt_multi = rdtsc() - t0;
        let finished = done.load(Ordering::Acquire);
        round.store(FILL_QUIT, Ordering::SeqCst);
        // done 是热身+计时轮累计值，减掉热身轮再显示。
        let timed_target = expected_aps * TIMED_ROUNDS as usize;
        let timed_done = finished.saturating_sub(expected_aps).min(timed_target);
        let multi_ms = dt_multi as f64 * 1000.0 / (TIMED_ROUNDS as f64 * tps as f64);
        let scaling = single_ms / multi_ms.max(0.001);
        tline!(
            report,
            screen,
            "fb fill {} cores: {:.2} ms ({:.1} MB/s, x{:.2} speedup, eff {:.0}%, ap {}/{}){}",
            n_cores,
            multi_ms,
            fb_bytes as f64 * tps as f64
                / (dt_multi as f64 / TIMED_ROUNDS as f64 * 1024.0 * 1024.0),
            scaling,
            scaling / n_cores as f64 * 100.0,
            timed_done,
            timed_target,
            if degraded { " TIMEOUT?" } else { "" }
        );
    }

    // ---- 瓶颈归因（启发式，看数字说话） ----
    // 传输 vs 引擎：Fill 不搬数据只考引擎，BLT 要搬数据。
    if blt_mbs < fb_mbs * 0.5 {
        tline!(
            report,
            screen,
            "verdict: transfer-bound (blit {:.0} << direct {:.0} MB/s)",
            blt_mbs,
            fb_mbs
        );
    } else {
        tline!(
            report,
            screen,
            "verdict: engine-bound (blit {:.0} ~= direct {:.0} MB/s)",
            blt_mbs,
            fb_mbs
        );
    }
    // CPU 换色 vs 上屏：播放管线里谁慢谁是瓶颈。
    if conv_mbs < blt_mbs {
        tline!(
            report,
            screen,
            "verdict: pipeline CPU-bound (convert {:.0} < blit {:.0} MB/s)",
            conv_mbs,
            blt_mbs
        );
    } else {
        tline!(
            report,
            screen,
            "verdict: pipeline blit-bound (convert {:.0} >= blit {:.0} MB/s)",
            conv_mbs,
            blt_mbs
        );
    }

    Ok(())
}

/// 按核 id 划分行区，尾核吃掉余数。
fn rows_for(id: usize, n: usize, h: usize) -> (usize, usize) {
    let per = h / n.max(1);
    let y0 = id * per;
    let y1 = if id + 1 >= n { h } else { y0 + per };
    (y0.min(h), y1.min(h))
}

/// u64 易失填充行区（显存是 WC 内存，必须 volatile，不许编译器合并/消掉）。
/// edition 2024：unsafe fn 体内显式 unsafe 块。
unsafe fn fill_rows(
    base: *mut u8,
    stride_bytes: usize,
    width_px: usize,
    y0: usize,
    y1: usize,
    pat: u64,
) {
    unsafe {
        let row_bytes = width_px * 4;
        for y in y0..y1 {
            let row = base.add(y * stride_bytes);
            let mut i = 0;
            while i + 8 <= row_bytes {
                (row.add(i) as *mut u64).write_volatile(pat);
                i += 8;
            }
        }
    }
}

struct FillCtx<'a> {
    mp: &'a MpServices,
    fb_base: *mut u8,
    stride_bytes: usize,
    width: usize,
    height: usize,
    n: usize,
    round: &'a AtomicUsize,
    done: &'a AtomicUsize,
}

/// 退出哨兵：BSP 写 round=QUIT 后 AP 跳出常驻循环，procedure 返回。
const FILL_QUIT: usize = usize::MAX;

/// AP 常驻工作循环：等新一轮轮次号，填自己行区，打卡。BSP 发 QUIT 后退出。
extern "efiapi" fn ap_fill_loop(arg: *mut c_void) {
    if arg.is_null() {
        return;
    }
    let ctx = unsafe { &*(arg as *const FillCtx) };
    let id = ctx.mp.who_am_i().unwrap_or(usize::MAX) % ctx.n.max(1);
    let (y0, y1) = rows_for(id, ctx.n, ctx.height);
    let mut my_round = 0usize;
    loop {
        let g = ctx.round.load(Ordering::Acquire);
        if g == FILL_QUIT {
            break;
        }
        if g == my_round {
            core::hint::spin_loop();
            continue;
        }
        my_round = g;
        unsafe {
            fill_rows(
                ctx.fb_base,
                ctx.stride_bytes,
                ctx.width,
                y0,
                y1,
                0x0044_5566_0044_5566,
            )
        };
        ctx.done.fetch_add(1, Ordering::SeqCst);
    }
}
