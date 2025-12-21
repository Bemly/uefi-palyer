use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use crate::error::Result;
use crate::video::ascii_font::FONT_8X16;
use crate::video::buffer::VideoMemoryRaw;

pub struct Screen {
    gop: ScopedProtocol<GraphicsOutput>,
    stdout: usize,
}

impl Screen {
    pub fn new() -> Result<Self> {
        let handle = get_handle_for_protocol::<GraphicsOutput>()?;
        let gop = open_protocol_exclusive::<GraphicsOutput>(handle)?;
        Ok(Self { gop, stdout: 0 })
    }

    pub fn draw_image(&mut self, width: u32, height: u32, pixels: &[BltPixel]) -> Result {
        // 我不知道为什么封装成这样了，但是它能工作！
        // 默认blt输出uefi::result::Result，这里?拆包然后Ok封装为crate::error::Result
        // ?外的Ok并不会影响错误抛出
        Ok(self.gop.blt(BltOp::BufferToVideo {
            buffer: pixels,
            src: BltRegion::Full,
            dest: (0, 0),
            dims: (width as usize, height as usize),
        })?)
    }

    pub fn clear(&mut self) -> Result {
        let info = self.gop.current_mode_info();
        let (width, height) = info.resolution();

        // 使用 VideoFill 操作，这比传输像素数组快得多
        Ok(self.gop.blt(BltOp::VideoFill {
            // 黑色像素：Red=0, Green=0, Blue=0
            color: BltPixel::new(0, 0, 0),
            dest: (0, 0),
            dims: (width, height),
        })?)
    }

    // 临时用来调试的 功能补全 不要用
    pub fn draw_string(&mut self, text: &str) {
        let mut x = 0;
        let y = 0;
        // 报错信息建议使用醒目的颜色：比如红底白字或黑底红字
        let fg = BltPixel::new(255, 255, 255); // 白色前景
        let bg = BltPixel::new(0, 0, 0);       // 黑色背景

        for c in text.chars() {
            // 1. 获取 ASCII 码索引 (0-127)
            let index = (c as usize) & 0x7F;

            // 2. 直接获取该字符的点阵行数据
            // glyph 的类型是 &[u8; 16]
            let glyph = &FONT_8X16[index];

            for row in 0..16 {
                // 获取这一行的 8 位像素数据
                let row_bits = glyph[row];

                for col in 0..8 {
                    // 3. 检查位。注意：最高位 (bit 7) 对应最左边的像素
                    // 使用 (row_bits >> (7 - col)) & 1 来提取
                    let is_fg = (row_bits >> (7 - col)) & 1 == 1;
                    let color = if is_fg { fg } else { bg };

                    // 4. 绘制像素
                    let _ = self.gop.blt(BltOp::VideoFill {
                        color,
                        dest: (x + col, y + row + self.stdout),
                        dims: (1, 1),
                    });
                }
            }

            // 5. 字符绘制完后，光标右移 8 像素
            x += 8;

            // 进阶提示：你可以在这里加一个检查，如果 x 超过屏幕宽度就自动换行
        }

        self.stdout += 18;
    }

    pub fn draw_all_mem_raw_zero_copy(&mut self, video: &mut VideoMemoryRaw, width: usize, height: usize) {
        // 获取下一帧的原始像素引用
        if let Some(pixel_slice) = video.next_frame() {
            // 直接绘制到屏幕
            // 假设你的屏幕分辨率和视频一致，从 (0,0) 开始画
            self.gop.blt(
                BltOp::BufferToVideo {
                    buffer: pixel_slice,
                    src: BltRegion::Full,
                    dest: (0, 0),
                    dims: (width, height),
                }
            ).ok();
        } else {
            video.rewind();
        }
    }
}