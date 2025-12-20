use core::fmt::Debug;
use uefi::boot::stall;
use core::time::Duration;
use log::error;


pub type Result<Output = (), ErrData = ()> = core::result::Result<Output, NyaStatus<ErrData>>;

#[derive(Debug)]
pub enum NyaStatus<Data: Debug = ()> {
    Uefi(uefi::Error<Data>),
    Qoi(qoi::Error),
    NotRegularFile,
    _Reserve,
}

impl From<uefi::Error> for NyaStatus {
    fn from(e: uefi::Error) -> Self { NyaStatus::Uefi(e) }
}

impl From<qoi::Error> for NyaStatus {
    fn from(e: qoi::Error) -> Self { NyaStatus::Qoi(e) }
}



// 统一的错误处理入口：打印并挂起
pub fn handle_fatal(err: NyaStatus) -> ! {
    match err {
        NyaStatus::Qoi(err) => error!("QOI error: {}", err),
        _ => error!("FATAL ERROR: {:?}", err),
    }

    error!("System will stall for 1 minute before returning.");

    // 停顿一分钟，方便用户看清屏幕上的错误
    stall(Duration::from_mins(2));

    // 在 UEFI 入口函数外通常无法返回，只能尝试重启或死循环
    panic!("Unrecoverable error occurred.");
}