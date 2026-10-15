use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive};
use uefi::proto::pi::mp::MpServices;

use crate::emit;
use crate::error::Result;
use crate::graphics::Screen;

// 注意必须要2个核心!

// 在 AP (应用处理器) 上执行的并行任务，必须使用 extern "efiapi" 以符合 UEFI 调用约定。
//
// 历史行为变更：原来 AP 里会自己 `open_protocol_exclusive::<GraphicsOutput>`
// 全屏刷红做“跑起来了”的视觉证明。菜单化后 GOP 由 `Screen` 在 BSP 常驻独占，
// AP 侧再 open 会直接失败（之前是 standalone 跑才没事），所以 AP 只做计数，
// 可视化改由 BSP 汇总后经 `screen` 输出。计数器本身就是“AP 响应了”的证明。
extern "efiapi" fn ap_count_task(arg: *mut c_void) {
    if arg.is_null() {
        return;
    }

    // 安全地转换指针并增加计数
    let counter = unsafe { &*(arg as *const AtomicUsize) };
    counter.fetch_add(1, Ordering::SeqCst);
}

pub fn run(screen: &mut Screen) -> Result {
    emit!(screen, "Hello, World!");
    emit!(screen, "=== Starting Multi-Processor (MP) Test ===");

    // 查找 MP Services 协议
    let mp_handle = match get_handle_for_protocol::<MpServices>() {
        Ok(handle) => handle,
        Err(_) => {
            emit!(
                screen,
                "Error: MP Services Protocol not supported by this firmware."
            );
            return Ok(());
        }
    };

    let mp_services = open_protocol_exclusive::<MpServices>(mp_handle)?;

    // 获取处理器详细信息
    let num_proc = mp_services.get_number_of_processors()?;

    emit!(screen, "Total processors detected: {}", num_proc.total);
    emit!(screen, "Enabled processors: {}", num_proc.enabled);

    // 如果系统只有一个核心，则没有 AP 需要启动
    if num_proc.enabled < 2 {
        emit!(
            screen,
            "Single-core system detected. Skipping parallel task execution."
        );
        return Ok(());
    }

    // 准备原子计数器
    let counter = AtomicUsize::new(0);
    let arg_ptr = &counter as *const _ as *mut c_void;

    emit!(
        screen,
        "Dispatching task to all Application Processors (APs)..."
    );

    // 启动所有 AP 并执行任务
    // - single_thread: false (表示并行执行)
    // - procedure: 任务函数指针
    // - procedure_argument: 传给任务函数的参数指针
    mp_services
        .startup_all_aps(false, ap_count_task, arg_ptr, None, None)
        .map_err(|e| e.status())?;

    // 最终验证结果
    // 注意：startup_all_aps 只在辅助核心 (AP) 上运行
    // 当前正在运行 main 函数的核心 (BSP) 不会执行该任务
    let final_count = counter.load(Ordering::SeqCst);
    let expected_count = num_proc.enabled - 1;

    emit!(screen, "Execution completed!");
    emit!(screen, "AP tasks finished: {}", final_count);
    emit!(screen, "Expected count: {}", expected_count);

    if final_count == expected_count {
        emit!(screen, "Result: SUCCESS - All APs responded.");
    } else {
        emit!(
            screen,
            "Result: MISMATCH - Some APs failed to increment the counter."
        );
    }

    // 原来这里 stall 2 分钟；菜单化后由统一的“按任意键回菜单”接管节奏。
    Ok(())
}
