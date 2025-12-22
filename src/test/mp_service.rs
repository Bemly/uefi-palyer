use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use uefi::prelude::*;
use core::time::Duration;
use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive};
use uefi::println;
use uefi::proto::pi::mp::MpServices;
use log::{error, info, warn};
use uefi::proto::console::gop::{BltOp, BltPixel, GraphicsOutput};


// 1. 定义在 AP (应用处理器) 上执行的并行任务
// 必须使用 extern "efiapi" 以符合 UEFI 调用约定
extern "efiapi" fn ap_count_task(arg: *mut c_void) {
    if arg.is_null() { return; }

    // 安全地转换指针并增加计数
    let counter = unsafe { &*(arg as *const AtomicUsize) };

    let handle =  get_handle_for_protocol::<GraphicsOutput>()
        .expect("Failed to open Graphics Output protocol");
    let mut gop = open_protocol_exclusive::<GraphicsOutput>(handle)
        .expect("Failed to open Graphics Output protocol");
    let info = gop.current_mode_info();
    let (width, height) = info.resolution();
    gop.blt(BltOp::VideoFill {
        // 黑色像素：Red=0, Green=0, Blue=0
        color: BltPixel::new(255, 0, 0),
        dest: (0, 0),
        dims: (width, height),
    }).expect("Failed to fill screen with black");
    counter.fetch_add(1, Ordering::SeqCst);
}

pub fn mp_service() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    // let mut screen = Screen::new().expect("Failed to create screen");
    // video_run(&mut screen).unwrap_or_else(|e| handle_fatal(e, &mut screen));

    println!("Hello, World!");

    info!("=== Starting Multi-Processor (MP) Test ===");

    // 2. 查找 MP Services 协议
    let mp_handle = match get_handle_for_protocol::<MpServices>() {
        Ok(handle) => handle,
        Err(_) => {
            error!("Error: MP Services Protocol not supported by this firmware.");
            return Status::UNSUPPORTED;
        }
    };

    let mp_services = open_protocol_exclusive::<MpServices>(mp_handle)
        .expect("Failed to open MP Services protocol");

    // 3. 获取处理器详细信息
    let num_proc = mp_services.get_number_of_processors()
        .expect("Failed to get processor count");

    info!("Total processors detected: {}", num_proc.total);
    info!("Enabled processors: {}", num_proc.enabled);

    // 如果系统只有一个核心，则没有 AP 需要启动
    if num_proc.enabled < 2 {
        warn!("Single-core system detected. Skipping parallel task execution.");
        return Status::SUCCESS;
    }

    // 4. 准备原子计数器
    let counter = AtomicUsize::new(0);
    let arg_ptr = &counter as *const _ as *mut c_void;

    info!("Dispatching task to all Application Processors (APs)...");

    // 5. 启动所有 AP 并执行任务
    // - single_thread: false (表示并行执行)
    // - procedure: 任务函数指针
    // - procedure_argument: 传给任务函数的参数指针
    if let Err(e) = mp_services.startup_all_aps(
        false,
        ap_count_task,
        arg_ptr,
        None,
        None
    ) {
        error!("Failed to start APs: {:?}", e);
        return e.status();
    }

    // 6. 最终验证结果
    // 注意：startup_all_aps 只在辅助核心 (AP) 上运行
    // 当前正在运行 main 函数的核心 (BSP) 不会执行该任务
    let final_count = counter.load(Ordering::SeqCst);
    let expected_count = num_proc.enabled - 1;

    info!("Execution completed!");
    info!("AP tasks finished: {}", final_count);
    info!("Expected count: {}", expected_count);

    if final_count == expected_count {
        info!("Result: SUCCESS - All APs responded.");
    } else {
        warn!("Result: MISMATCH - Some APs failed to increment the counter.");
    }

    boot::stall(Duration::from_mins(2));
    Status::SUCCESS
}


