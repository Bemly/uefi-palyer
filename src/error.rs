use alloc::format;
use core::fmt::Debug;
use uefi::boot::stall;
use core::time::Duration;
use crate::graphics::Screen;

pub type Result<Output = (), ErrData = ()> = core::result::Result<Output, NyaStatus<ErrData>>;

#[derive(Debug)]
pub enum NyaStatus<Data: Debug = ()> {
    Uefi(uefi::Error<Data>),
    FromStrWithBufError,
    Qoi(qoi::Error),
    NotRegularFile,
    _Reserve,
}

impl From<uefi::Error> for NyaStatus {
    fn from(e: uefi::Error) -> Self { NyaStatus::Uefi(e) }
}

// 直接转换状态
impl From<uefi::Status> for NyaStatus {
    fn from(s: uefi::Status) -> Self {
        NyaStatus::Uefi(uefi::Error::new(s, ()))
    }
}

impl From<qoi::Error> for NyaStatus {
    fn from(e: qoi::Error) -> Self { NyaStatus::Qoi(e) }
}



// 统一的错误处理入口：打印并挂起
pub fn handle_fatal(err: NyaStatus, screen: &mut Screen) -> ! {
    let _ = screen.clear();

    screen.draw_string("KERNEL PANIC!");

    match err {
        NyaStatus::Qoi(err) => screen.draw_string(format!("QOI error: {}", err).as_str()),
        _ => screen.draw_string(format!("FATAL ERROR: {:?}", err).as_str()),
    }

    screen.draw_string("System will stall for 1 minute before returning.");

    // 停顿一分钟，方便用户看清屏幕上的错误
    stall(Duration::from_mins(2));

    // 在 UEFI 入口函数外通常无法返回，只能尝试重启或死循环
    panic!("Unrecoverable error occurred.");
}