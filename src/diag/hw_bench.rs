//! 整机硬件测试：SIMD 支持度实测 + 屏幕/GOP/算力基准，跑完落盘。
//!
//! - SSE/AVX：CPUID 查位 + **真执行一条向量指令验算**（光看 CPUID 不叫测试）。
//!   AVX 执行前必须过 `check_avx_support`（硬件 + OSXSAVE + XCR0），否则 #UD。
//! - 屏幕刷新：GOP VideoFill / BufferToVideo 整屏帧率。
//! - GOP 速度：framebuffer 裸指针直写带宽。
//! - 算力：memcpy 带宽 + 整数流水线吞吐。
//! - 落盘：启动卷根目录写 `HWBENCHnn.TXT`（从 U 盘启动时就是 U 盘），
//!   文件名自动递增，连测多台机器不互相覆盖。
//!
//! x86_64 only：CPUID / RDTSC / SSE-AVX 内在函数。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};
use core::arch::asm;
use core::time::Duration;

use uefi::boot::{self, get_handle_for_protocol, open_protocol_exclusive};
use uefi::proto::pi::mp::MpServices;

use crate::diag::avx_checker::check_avx_support;
use crate::diag::common::save_report;
use crate::emit;
use crate::error::Result;
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

/// SSE2 整数加法实测。
///
/// 约束（全是反汇编逼出来的）：
/// 1. 只测整数向量：浮点向量在此 target 上曾经全军覆没（标量化/错常量），
///    而 `.cargo/config.toml` 整 crate 摘软浮点后整数路径已验证发射；
///    浮点执行不在本测试覆盖内（CPUID+XCR0 矩阵照常报告支持情况）。
/// 2. 加法本身必须用 `asm!` 显式 `xmm_reg`：内在函数版的 load/add/store
///    会被 LLVM 整链标量化（二进制零条向量指令——整数得假 PASS）。
///    内在函数只配构造操作数（构造路径怎么编都无所谓）。
/// 3. `#[target_feature]` 保留做双保险（compile-time 门控 + 文档作用），
///    真正的 ABI 前提由 `.cargo/config.toml` 的 crate 级 flags 保证。
#[target_feature(enable = "sse2")]
unsafe fn sse_int_add_test(seed: u32) -> bool {
    // edition 2024：unsafe fn 体内也要显式 unsafe 块。
    unsafe {
        // lane 值运行时派生；u128 纯整数类型不进向量 legalize
        //（__m128i 会在 asm 约束处触发 getCopyFromParts 后端错误），
        // paddd 只认 bit pattern，lane 划分靠移位。
        let s = seed;
        let a: u128 = (s as u128)
            | ((s.wrapping_add(1) as u128) << 32)
            | ((s.wrapping_add(2) as u128) << 64)
            | ((s.wrapping_add(3) as u128) << 96);
        let b: u128 = 10 | (20u128 << 32) | (30u128 << 64) | (40u128 << 96);
        let mut acc = a;
        // Intel 语法：paddd acc, rhs  <=>  acc += rhs（lane 级 wrapping，与 wrapping_add 同语义）。
        asm!(
            "paddd {acc}, {rhs}",
            acc = inlateout(xmm_reg) acc,
            rhs = in(xmm_reg) b,
            options(pure, nomem, nostack, preserves_flags),
        );
        let want: u128 = ((s as i32).wrapping_add(10) as u32 as u128)
            | (((s.wrapping_add(1) as i32).wrapping_add(20) as u32 as u128) << 32)
            | (((s.wrapping_add(2) as i32).wrapping_add(30) as u32 as u128) << 64)
            | (((s.wrapping_add(3) as i32).wrapping_add(40) as u32 as u128) << 96);
        acc == want
    }
}

/// AVX2 整数加法实测（256 位整数要 AVX2，调用前必须同时满足
/// `check_avx_support()` 与 CPUID.07h:EBX.AVX2）。约束同 SSE 版。
#[target_feature(enable = "avx2")]
unsafe fn avx2_int_add_test(seed: u32) -> bool {
    use core::arch::x86_64::{_mm256_set_epi32, _mm256_storeu_si256};
    unsafe {
        let a = _mm256_set_epi32(
            seed.wrapping_add(7) as i32,
            seed.wrapping_add(6) as i32,
            seed.wrapping_add(5) as i32,
            seed.wrapping_add(4) as i32,
            seed.wrapping_add(3) as i32,
            seed.wrapping_add(2) as i32,
            seed.wrapping_add(1) as i32,
            seed as i32,
        );
        let b = _mm256_set_epi32(8, 7, 6, 5, 4, 3, 2, 1);
        let mut acc = a;
        // VEX 3 操作数：vpaddd acc, lhs, rhs  <=>  acc = lhs + rhs。
        asm!(
            "vpaddd {acc}, {lhs}, {rhs}",
            acc = inlateout(ymm_reg) acc,
            lhs = in(ymm_reg) a,
            rhs = in(ymm_reg) b,
            options(pure, nomem, nostack, preserves_flags),
        );
        let mut out = [0i32; 8];
        _mm256_storeu_si256(out.as_mut_ptr() as *mut _, acc);
        let s = seed;
        out == [
            (s as i32).wrapping_add(1),
            (s.wrapping_add(1) as i32).wrapping_add(2),
            (s.wrapping_add(2) as i32).wrapping_add(3),
            (s.wrapping_add(3) as i32).wrapping_add(4),
            (s.wrapping_add(4) as i32).wrapping_add(5),
            (s.wrapping_add(5) as i32).wrapping_add(6),
            (s.wrapping_add(6) as i32).wrapping_add(7),
            (s.wrapping_add(7) as i32).wrapping_add(8),
        ]
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
    // ---- SIMD 功能实测（纯整数向量；浮点向量在此 target 上必被软浮点化，不测） ----
    tline!(
        report,
        screen,
        "  SSE2 int add : {}",
        if f.sse2 && unsafe { sse_int_add_test(rdtsc() as u32) } {
            "PASS"
        } else {
            "FAIL/SKIP"
        }
    );
    if check_avx_support() && f.avx2_hw {
        let ok = unsafe { avx2_int_add_test(rdtsc() as u32) };
        tline!(report, screen, "  AVX2 int add : {}", if ok { "PASS" } else { "FAIL" });
    } else {
        tline!(report, screen, "  AVX2 int add : SKIP (need AVX+XCR0+AVX2-hw)");
    }

    // ---- 基准 ----
    let tps = calibrate_ticks_per_sec();
    tline!(report, screen, "tsc: {} ticks/sec", tps);

    // ---- GOP 压测（单/多核、带宽、瓶颈归因，见 gop_bench） ----
    crate::diag::gop_bench::run_section(&mut report, screen, tps)?;

    // ---- 内存拷贝带宽 ----
    // 注意：src/dst 固定时 LLVM 会把 20 次折叠成 1 次（曾测出 96 万 MB/s 的笑话），
    // 每轮 black_box 指针逼真干 20 次。
    const MEM_N: u64 = 20;
    const MEM_LEN: usize = 4 * 1024 * 1024;
    let src = vec![0xA5u8; MEM_LEN];
    let mut dst = vec![0u8; MEM_LEN];
    let t0 = rdtsc();
    for _ in 0..MEM_N {
        let s = core::hint::black_box(src.as_ptr());
        let d = core::hint::black_box(dst.as_mut_ptr());
        unsafe {
            core::ptr::copy_nonoverlapping(s, d, MEM_LEN);
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

    // ---- 整数流水线 ----
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

    // ---- 落盘：文件名自动递增（HWBENCH00.TXT、HWBENCH01.TXT…），
    // 连测多台机器不互相覆盖 ----
    let body = report.join("\n") + "\n";
    match save_report("HWBENCH", body.as_bytes()) {
        Ok(s) => tline!(report, screen, "{}", s),
        Err(e) => tline!(report, screen, "SAVE FAILED: {:?}", e),
    }

    emit!(screen, "-- bench done --");
    Ok(())
}
