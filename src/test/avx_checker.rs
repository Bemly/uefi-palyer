use core::arch::x86_64::{__cpuid, _xgetbv};

pub fn check_avx_support() -> bool {
    unsafe {
        // 1. 检查 CPUID.01h:ECX.AVX[bit 28] 和 CPUID.01h:ECX.OSXSAVE[bit 27]
        let cpuid_res = __cpuid(1);

        // Bit 27: OSXSAVE (系统是否支持扩展状态)
        // Bit 28: AVX (硬件是否支持 AVX)
        let osxsave_mask = 1 << 27;
        let avx_mask = 1 << 28;

        if (cpuid_res.ecx & avx_mask) == 0 {
            return false; // 硬件压根不支持
        }

        if (cpuid_res.ecx & osxsave_mask) == 0 {
            return false; // 固件未开启 OSXSAVE
        }

        // 2. 检查 XCR0 寄存器 (Extended Control Register)
        // 使用 xgetbv 指令，索引为 0
        // Bit 1: SSE 状态, Bit 2: AVX 状态
        let xcr0 = _xgetbv(0);
        let avx_state_mask = 1 << 2;

        (xcr0 & avx_state_mask) != 0
    }
}

// 在 main 中调用
pub fn print_avx_status() {
    if check_avx_support() {
        // 此时可以安全使用 AVX 指令
        log::info!("AVX is enabled and safe to use.");
    } else {
        log::warn!("AVX is NOT available. Falling back to SSE/u64.");
    }
}