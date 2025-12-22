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

// 垫底的错误处理，至少是安全的输出内容（大雾
pub fn handle_fatal(err: NyaStatus, mut screen: Screen) -> ! {
    let _ = screen.clear();

    macro_rules! println {
        ($($arg:tt)*) => { screen.draw_str(&alloc::format!($($arg)*)) }
    }


    println!("KERNEL PANIC!");

    match err {
        NyaStatus::Qoi(err) => println!("QOI error: {}", err),
        _ => println!("FATAL ERROR: {:?}", err),
    }

    println!("System will stall for 1 minute before returning.");

    stall(Duration::from_mins(2));

    panic!("Unrecoverable error occurred.");
}

#[allow(unused)] // 宏的大手
pub fn _handle_fatal(err: NyaStatus, mut screen: Screen) -> ! {
    use embedded_graphics::{
        prelude::{Point, RgbColor},
        mono_font::{
            ascii::FONT_10X20, MonoTextStyle
        },
        pixelcolor::Rgb888,
        text::Text, Drawable
    };
    use embedded_graphics_gop::fb::FbDrawTarget;

    // emg 会锁住GOP协议,最好还是用自己的,这里崩溃导出 所以让出所有权
    // 1.Screen生命周期不好处理,2.每次新建画布都会覆盖掉内容=>故这里为高耦合设计
    let mut canva = FbDrawTarget::new(screen.get_gop());
    const OFFSET: i32 = 18;
    let mut offset = OFFSET;

    macro_rules! println {
        ($($arg:tt)*) => {{
            let _ = Text::new(&alloc::format!($($arg)*),
                Point::new(0, offset),
                MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE)
            ).draw(&mut canva);
            offset += OFFSET;
        }}
    }

    match err {
        NyaStatus::Qoi(err) => println!("QOI error: {}", err),
        _ => println!("FATAL ERROR: {:?}", err),
    };

    println!("System will stall for 1 minute before returning.");

    // 停顿一分钟，方便用户看清屏幕上的错误
    stall(Duration::from_mins(2));

    // 在 UEFI 入口函数外通常无法返回，只能尝试重启或死循环
    panic!("Unrecoverable error occurred.");
}