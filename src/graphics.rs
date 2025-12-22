use core::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_prefetch, _mm_stream_si128, _MM_HINT_T0};
use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput, Mode};
use crate::error::Result;
use crate::video::ascii_font::FONT_8X16;
use crate::video::decoder::VideoMemoryRaw;

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


    /// 高性能显存直接写入
    pub fn draw_fast_direct_copy(
        &mut self,
        video: &mut VideoMemoryRaw,
        width: usize,
        height: usize
    ) {
        // 1. 获取下一帧
        let pixel_slice = match video.next_frame() {
            Some(slice) => slice,
            None => {
                video.rewind();
                return;
            }
        };

        // 2. 先提取 ModeInfo（此时 gop 会被借用，但在这一行结束后就会释放）
        let mode_info = self.gop.current_mode_info();
        let stride = mode_info.stride();

        // 3. 再获取 FrameBuffer（此时 gop 被独占借用）
        let mut fb = self.gop.frame_buffer();
        let dest_ptr = fb.as_mut_ptr();

        // 4. 执行内存拷贝
        unsafe {
            if stride == width {
                // 全局一次性拷贝
                core::ptr::copy_nonoverlapping(
                    pixel_slice.as_ptr() as *const u8,
                    dest_ptr,
                    width * height * 4
                );
            } else {
                // 考虑 Stride 的逐行拷贝
                let src_ptr = pixel_slice.as_ptr() as *const u8;
                for y in 0..height {
                    let row_src = src_ptr.add(y * width * 4);
                    let row_dest = dest_ptr.add(y * stride * 4);
                    core::ptr::copy_nonoverlapping(row_src, row_dest, width * 4);
                }
            }
        }
    }


    #[inline]
    pub fn draw_u64_optimized(
        &mut self,
        video: &mut VideoMemoryRaw,
        width: usize,
        height: usize,
        stride: usize,
        is_continuous: bool,
        dest_ptr: *mut u8
    ) {
        let pixel_slice = match video.next_frame() {
            Some(slice) => slice,
            None => { video.rewind(); return; }
        };

        let src_ptr = pixel_slice.as_ptr() as *const u8;

        unsafe {
            if is_continuous {
                // 全屏连续拷贝
                let total_bytes = width * height * 4;
                self.u64_fast_copy(src_ptr, dest_ptr, total_bytes);
            } else {
                // 按行拷贝（处理 Stride）
                let row_bytes = width * 4;
                let stride_bytes = stride * 4;
                for y in 0..height {
                    let row_src = src_ptr.add(y * row_bytes);
                    let row_dest = dest_ptr.add(y * stride_bytes);
                    self.u64_fast_copy(row_src, row_dest, row_bytes);
                }
            }
        }
    }

    #[inline(always)]
    unsafe fn u64_fast_copy(&self, src: *const u8, dst: *mut u8, len: usize) {
        let mut i = 0;

        // 1. 核心循环：展开 8 倍 (每次处理 64 字节)
        // 使用 u64 (8字节) * 8 = 64字节，正好匹配一个 Cache Line
        while i + 64 <= len {
            // 使用 read_unaligned 防止 src 不对齐，write_volatile 确保写入显存不经过缓存
            let s0 = (src.add(i) as *const u64).read_unaligned();
            let s1 = (src.add(i + 8) as *const u64).read_unaligned();
            let s2 = (src.add(i + 16) as *const u64).read_unaligned();
            let s3 = (src.add(i + 24) as *const u64).read_unaligned();
            let s4 = (src.add(i + 32) as *const u64).read_unaligned();
            let s5 = (src.add(i + 40) as *const u64).read_unaligned();
            let s6 = (src.add(i + 48) as *const u64).read_unaligned();
            let s7 = (src.add(i + 56) as *const u64).read_unaligned();

            (dst.add(i) as *mut u64).write_volatile(s0);
            (dst.add(i + 8) as *mut u64).write_volatile(s1);
            (dst.add(i + 16) as *mut u64).write_volatile(s2);
            (dst.add(i + 24) as *mut u64).write_volatile(s3);
            (dst.add(i + 32) as *mut u64).write_volatile(s4);
            (dst.add(i + 40) as *mut u64).write_volatile(s5);
            (dst.add(i + 48) as *mut u64).write_volatile(s6);
            (dst.add(i + 56) as *mut u64).write_volatile(s7);

            i += 64;
        }

        // 2. 补漏循环：每次 8 字节
        while i + 8 <= len {
            let val = (src.add(i) as *const u64).read_unaligned();
            (dst.add(i) as *mut u64).write_volatile(val);
            i += 8;
        }

        // 3. 尾部处理：处理不足 8 字节的散碎字节
        while i < len {
            dst.add(i).write_volatile(src.add(i).read());
            i += 1;
        }
    }

    pub fn draw_u64_optimized_loop(&mut self, video: &mut VideoMemoryRaw, width: usize, height: usize) {
        let mode_info = self.gop.current_mode_info();
        let stride = mode_info.stride();
        let mut fb = self.gop.frame_buffer();
        let is_continuous = stride == width;
        let dest_ptr = fb.as_mut_ptr();

        loop {
            self.draw_u64_optimized(video, width, height, stride, is_continuous, dest_ptr);
        }
    }
}

