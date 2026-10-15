use embedded_graphics::mono_font::jis_x0201::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_graphics_gop::BltDrawTarget;
use u8g2_fonts::fonts::u8g2_font_boutique_bitmap_7x7_t_chinese3;
use u8g2_fonts::U8g2TextStyle;
use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;

use crate::error::{NyaStatus, Result};

/// 特例：需要自己独占 GOP（含强制切 800x600），所以签名是 `run() -> Result`，
/// 由菜单先 `drop(Screen)` 再调用，跑完重建 `Screen`。见 [`crate::diag::registry::Entry`]。
pub fn run() -> Result {
    // Get the first available handle for the GOP.
    let handle = boot::get_handle_for_protocol::<GraphicsOutput>()?;
    // Open the protocol in exclusive mode (exclusive mode is for applications, other modes are
    // intended for drivers)
    let mut protocol = boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;
    // Configure protocol here if desired, for example
    let mode = protocol
        .modes()
        .find(|m| m.info().resolution() == (800, 600))
        .ok_or(NyaStatus::_Debug(alloc::string::String::from(
            "800x600 GOP mode not found",
        )))?;
    protocol.set_mode(&mode)?;

    // Create the draw target utilizing the configured protocol.
    let mut target =
        BltDrawTarget::new(&mut protocol).map_err(|e| NyaStatus::_Debug(alloc::format!("{:?}", e)))?;
    // Make it double-buffered
    target
        .double_buffer(false)
        .map_err(|e| NyaStatus::_Debug(alloc::format!("{:?}", e)))?;

    // ...draw on it...
    Text::new("ｱｲｳｴｵ, ASCII_JP
    1x   0   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    2x  SP   !  \"   #   $   %   &   '   (   )   *   +   ,   -   .   /
    3x   0   1   2   3   4   5   6   7   8   9   :   ;   <   =   >   ?
    4x   @   A   B   C   D   E   F   G   H   I   J   K   L   M   N   O
    5x   P   Q   R   S   T   U   V   W   X   Y   Z   [   ¥   ]   ^   _
    6x   `   a   b   c   d   e   f   g   h   i   j   k   l   m   n   o
    7x   p   q   r   s   t   u   v   w   x   y   z   {   |   }   ‾   DEL
    Ax       ｡   ｢   ｣   ､   ･   ｦ   ｧ   ｨ   ｩ   ｪ   ｫ   ｬ   ｭ   ｮ   ｯ
    Bx   ｰ   ｱ   ｲ   ｳ   ｴ   ｵ   ｶ   ｷ   ｸ   ｹ   ｺ   ｻ   ｼ   ｽ   ｾ   ｿ
    Cx   ﾀ   ﾁ   ﾂ   ﾃ   ﾄ   ﾅ   ﾆ   ﾇ   ﾈ   ﾉ   ﾊ   ﾋ   ﾌ   ﾍ   ﾎ   ﾏ
    Dx   ﾐ   ﾑ   ﾒ   ﾓ   ﾔ   ﾕ   ﾖ   ﾗ   ﾘ   ﾙ   ﾚ   ﾛ   ﾜ   ﾝ   ﾞ   ﾟ  ",
        Point::new(0,15),
        MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE)
    ).draw(&mut target).map_err(|e| NyaStatus::_Debug(alloc::format!("{:?}", e)))?;

    Text::new("何意味", Point::new(5,30),
              U8g2TextStyle::new(u8g2_font_boutique_bitmap_7x7_t_chinese3, Rgb888::WHITE)
    ).draw(&mut target).map_err(|e| NyaStatus::_Debug(alloc::format!("{:?}", e)))?;

    // Transfer changes to the framebuffer
    target
        .commit()
        .map_err(|e| NyaStatus::_Debug(alloc::format!("{:?}", e)))?;

    Ok(())
}
