//! 整机硬件测试：SIMD 支持度实测 + 屏幕/GOP/算力基准，跑完落盘。
//!
//! - SSE/AVX：CPUID 查位 + **真执行一条内在函数验算**（光看 CPUID 不叫测试）。
//!   AVX 执行前必须过 `check_avx_support`（硬件 + OSXSAVE + XCR0），否则 #UD。
//! - 屏幕刷新：GOP VideoFill / BufferToVideo 整屏帧率。
//! - GOP 速度：framebuffer 裸指针直写带宽。
//! - 算力：memcpy 带宽 + 整数流水线吞吐。
//! - 落盘：启动卷根目录写 `HWBENCH.TXT`（从 U 盘启动时就是 U 盘）。
//!
//! x86_64 only：CPUID / RDTSC / SSE-AVX 内在函数。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::{
    __cpuid, __cpuid_count, _mm_add_epi32, _mm_add_ps, _mm_set_epi32, _mm_set_ps,
    _mm_storeu_ps, _mm_storeu_si128, _xgetbv,
};
use core::time::Duration;

use uefi::boot::{self, get_handle_for_protocol, open_protocol_exclusive};
use uefi::cstr16;
use uefi::proto::console::gop::BltPixel;
use uefi::proto::pi::mp::MpServices;

use crate::diag::avx_checker::check_avx_support;
use crate::emit;
use crate::error::Result;
use crate::fs::Fs;
use crate::graphics::Screen;

/// 双记一行：屏显（+串口）与报告正文各一份。
fn line(report: &mut Vec<String>, screen: &mut Screen, s: String) {
    screen.draw_str(&s);
    report.push(s);
}

macro_rules! tline {
    ($report:expr, $screen:expr, $($arg:tt)*) => {{
        let s = alloc::format!($($arg)*);
        log::info!("{}", s);
        $screen.draw_str(&s);
        $report.push(s);
    }};
}

// ---------- CPUID ----------

struct SimdCpuid {
    sse: bool,
    sse2: bool,
    sse3: bool,
    pclmulqdq: bool,
    ssse3: bool,
    fma: bool,
    sse41: bool,
    sse42: bool,
    aes: bool,
    osxsave: bool,
    avx_hw: bool,
    avx2_hw: bool,
}

fn cpuid_simd() -> SimdCpuid {
    // __cpuid 在新版 Rust 里是 safe fn，无需 unsafe。
    let c1 = __cpuid(1);
    let c7 = __cpuid_count(7, 0);
    SimdCpuid {
        sse: c1.edx & (1 << 25) != 0,
        sse2: c1.edx & (1 << 26) != 0,
        sse3: c1.ecx & (1 << 0) != 0,
        pclmulqdq: c1.ecx & (1 << 1) != 0,
        ssse3: c1.ecx & (1 << 9) != 0,
        fma: c1.ecx & (1 << 12) != 0,
        sse41: c1.ecx & (1 << 19) != 0,
        sse42: c1.ecx & (1 << 20) != 0,
        aes: c1.ecx & (1 << 25) != 0,
        osxsave: c1.ecx & (1 << 27) != 0,
        avx_hw: c1.ecx & (1 << 28) != 0,
        avx2_hw: c7.ebx & (1 << 5) != 0,
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "YES"
    } else {
        "no"
    }
}

// ---------- 功能实测 ----------

fn sse_int_add_test() -> bool {
    unsafe {
        let a = _mm_set_epi32(1, 1, 1, 1);
        let b = _mm_set_epi32(2, 2, 2, 2);
        let c = _mm_add_epi32(a, b);
        let mut out = [0i32; 4];
        _mm_storeu_si128(out.as_mut_ptr() as *mut _, c);
        out == [3, 3, 3, 3]
    }
}

fn sse_float_add_test() -> [f32; 4] {
    // 注：QEMU TCG 下曾观测到返回 [2.0, 0.0, 4.0, 0.0]（奇 lane 归零），
    // 属模拟器浮点 lane quirk；真机以实际输出为准，FAIL 时看 got= 值。
    unsafe {
        let a = _mm_set_ps(4.0, 3.0, 2.0, 1.0);
        let b = _mm_set_ps(1.0, 1.0, 1.0, 1.0);
        let c = _mm_add_ps(a, b);
        let mut out = [0f32; 4];
        _mm_storeu_ps(out.as_mut_ptr(), c);
        out
    }
}

/// AVX 版必须开 target_feature 单独编译，调用前必须已过可用性检查。
/// 用 float 版（AVX1 即可；整数 256 位要 AVX2，另起检测位）。
#[target_feature(enable = "avx")]
unsafe fn avx_float_add_test() -> bool {
    use core::arch::x86_64::{_mm256_add_ps, _mm256_set_ps, _mm256_storeu_ps};
    // edition 2024：unsafe fn 体内也要显式 unsafe 块。
    unsafe {
        let a = _mm256_set_ps(8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0);
        let b = _mm256_set_ps(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let c = _mm256_add_ps(a, b);
        let mut out = [0f32; 8];
        _mm256_storeu_ps(out.as_mut_ptr(), c);
        out == [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]
    }
}

// ---------- 计时 ----------

#[inline(always)]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// 100ms Stall 校准 TSC 频率。QEMU/真机都可用，TCG 下测出来偏小但自洽。
fn calibrate_ticks_per_sec() -> u64 {
    boot::stall(Duration::from_millis(100));
    let s = rdtsc();
    boot::stall(Duration::from_millis(100));
    let e = rdtsc();
    (e - s) * 10
}

// ---------- 主入口 ----------

pub fn run(screen: &mut Screen) -> Result {
    let mut report: Vec<String> = Vec::new();

    tline!(report, screen, "== HW Bench ==");
    tline!(report, screen, "target: x86_64-uefi, opt: release");

    // ---- CPU 基本信息 ----
    let cpuid = raw_cpuid::CpuId::new();
    if let Some(brand) = cpuid.get_processor_brand_string() {
        let s = format!("cpu: {}", brand.as_str().trim());
        line(&mut report, screen, s.clone());
        log::info!("{}", s);
    }
    if let Ok(h) = get_handle_for_protocol::<MpServices>() {
        if let Ok(mp) = open_protocol_exclusive::<MpServices>(h) {
            if let Ok(n) = mp.get_number_of_processors() {
                tline!(
                    report,
                    screen,
                    "cores: total {} enabled {}",
                    n.total,
                    n.enabled
                );
            }
        }
    }

    // ---- SIMD 支持度 ----
    let f = cpuid_simd();
    tline!(report, screen, "-- SIMD cpuid --");
    for (name, on) in [
        ("SSE", f.sse),
        ("SSE2", f.sse2),
        ("SSE3", f.sse3),
        ("SSSE3", f.ssse3),
        ("SSE4.1", f.sse41),
        ("SSE4.2", f.sse42),
        ("PCLMULQDQ", f.pclmulqdq),
        ("AES-NI", f.aes),
        ("FMA", f.fma),
        ("AVX(hw)", f.avx_hw),
        ("AVX2(hw)", f.avx2_hw),
        ("OSXSAVE", f.osxsave),
    ] {
        tline!(report, screen, "  {:<10} {}", name, yn(on));
    }

    // XCR0：只有 OSXSAVE 开了才敢读，否则 #UD。
    if f.osxsave {
        let xcr0 = unsafe { _xgetbv(0) };
        tline!(
            report,
            screen,
            "xcr0: SSE-state={} AVX-state={} XCR0={:#x}",
            yn(xcr0 & (1 << 1) != 0),
            yn(xcr0 & (1 << 2) != 0),
            xcr0
        );
    } else {
        tline!(report, screen, "xcr0: SKIPPED (no OSXSAVE)");
    }

    // ---- SIMD 功能实测 ----
    tline!(report, screen, "-- SIMD exec test --");
    tline!(
        report,
        screen,
        "  SSE2 int add : {}",
        if f.sse2 && sse_int_add_test() {
            "PASS"
        } else {
            "FAIL/SKIP"
        }
    );
    if f.sse {
        let got = sse_float_add_test();
        if got == [2.0, 3.0, 4.0, 5.0] {
            tline!(report, screen, "  SSE float add: PASS");
        } else {
            tline!(report, screen, "  SSE float add: FAIL got={:?}", got);
        }
    } else {
        tline!(report, screen, "  SSE float add: SKIP (no SSE)");
    }
    if check_avx_support() {
        let ok = unsafe { avx_float_add_test() };
        tline!(report, screen, "  AVX float add: {}", if ok { "PASS" } else { "FAIL" });
    } else {
        tline!(report, screen, "  AVX float add: SKIP (not usable)");
    }

    // ---- 基准 ----
    let tps = calibrate_ticks_per_sec();
    tline!(report, screen, "tsc: {} ticks/sec", tps);

    let (w, h) = screen.get_gop().current_mode_info().resolution();
    let stride = screen.get_gop().current_mode_info().stride();
    tline!(report, screen, "gop: {}x{} stride {}", w, h, stride);

    // 1) 整屏 VideoFill 刷新
    const FILL_N: u64 = 10;
    let t0 = rdtsc();
    for _ in 0..FILL_N {
        screen.clear()?;
    }
    let dt = rdtsc() - t0;
    tline!(
        report,
        screen,
        "screen VideoFill: {:.1} fps ({:.2} ms/frame)",
        (FILL_N as f64 * tps as f64) / dt as f64,
        dt as f64 * 1000.0 / (FILL_N as f64 * tps as f64)
    );

    // 2) 整屏 BufferToVideo
    let pixels = vec![BltPixel::new(0x11, 0x22, 0x33); w * h];
    const BLT_N: u64 = 10;
    let t0 = rdtsc();
    for _ in 0..BLT_N {
        screen.draw_image(w as u32, h as u32, &pixels)?;
    }
    let dt = rdtsc() - t0;
    let bytes = BLT_N * (w * h * 4) as u64;
    tline!(
        report,
        screen,
        "gop BufferToVideo: {:.1} fps, {:.1} MB/s",
        (BLT_N as f64 * tps as f64) / dt as f64,
        bytes as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0)
    );
    drop(pixels);

    // 3) framebuffer 裸写带宽
    let fb_bytes = w * h * 4;
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
    let bytes = FB_N * fb_bytes as u64;
    tline!(
        report,
        screen,
        "gop direct-write: {:.1} MB/s",
        bytes as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0)
    );

    // 4) 内存拷贝带宽
    const MEM_N: u64 = 20;
    const MEM_LEN: usize = 4 * 1024 * 1024;
    let src = vec![0xA5u8; MEM_LEN];
    let mut dst = vec![0u8; MEM_LEN];
    let t0 = rdtsc();
    for _ in 0..MEM_N {
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), MEM_LEN);
        }
    }
    let dt = rdtsc() - t0;
    let bytes = MEM_N * MEM_LEN as u64;
    tline!(
        report,
        screen,
        "memcpy: {:.1} MB/s (chk={})",
        bytes as f64 * tps as f64 / (dt as f64 * 1024.0 * 1024.0),
        dst[MEM_LEN - 1]
    );

    // 5) 整数流水线
    const OPS: u64 = 20_000_000;
    let mut acc: u64 = 0x1234_5678;
    let t0 = rdtsc();
    for i in 0..OPS {
        acc = acc.wrapping_add(i).wrapping_mul(0x9E37_79B9);
    }
    let dt = rdtsc() - t0;
    core::hint::black_box(acc);
    tline!(
        report,
        screen,
        "int ALU: {:.1} Mops/s (chk={:#x})",
        OPS as f64 * tps as f64 / (dt as f64 * 1_000_000.0),
        acc & 0xFFFF
    );

    // ---- 落盘 ----
    let body = report.join("\n") + "\n";
    match Fs::new().and_then(|mut fs| fs.write_file(cstr16!("HWBENCH.TXT"), body.as_bytes())) {
        Ok(()) => tline!(report, screen, "saved: HWBENCH.TXT ({} bytes)", body.len()),
        Err(e) => tline!(report, screen, "SAVE FAILED: {:?}", e),
    }

    emit!(screen, "-- bench done --");
    Ok(())
}
