use alloc::vec;
use alloc::vec::Vec;
use qoi::Header;
use uefi::proto::console::gop::BltPixel;
use uefi::proto::media::file::{File, FileInfo, RegularFile};
use uefi::Status;
use crate::as_u8_slice_mut;
use crate::error::Result;

/// 3阶段: Qoi -> Raw -> Blt
/// Qoi压缩的数据
pub struct QoiFrameBuffer(pub Vec<u8>);

/// 解码后的数据
pub struct RawFrameBuffer {
    pub pixels: Vec<u8>,
    pub header: Header,
}

/// 交给GOP的数据
pub struct BltFrameBuffer(pub Vec<BltPixel>);



impl QoiFrameBuffer {
    pub fn new(size: usize) -> Self {
        Self { 0: vec![0u8; size] }
    }

}

impl RawFrameBuffer {
    pub fn new(size: usize) -> Self {
        Self {
            pixels: vec![0u8; size],
            header: Default::default(),
        }
    }

    #[inline]
    pub fn get_size(&self) -> usize {
        (self.header.width * self.header.height) as usize
    }
}

impl BltFrameBuffer {
    pub fn new(size: usize) -> Self {
        Self { 0: vec![BltPixel::new(0, 0, 0); size]}
    }
}


///////// 全部写入内存
pub struct VideoMemory {
    pub data: Vec<u8>,
    pub cursor: usize,
}

impl VideoMemory {
    pub fn new(mut file: RegularFile) -> Result<Self> {
        let mut info_buf = vec![0u8; 128];
        let info = loop {
            match file.get_info::<FileInfo>(&mut info_buf) {
                Ok(info) => break info,
                Err(e) if e.status() == Status::BUFFER_TOO_SMALL =>
                    if let Some(size) = *e.data() {
                        info_buf.resize(size, 0)
                    },
                // 其他状态的Option均为None,这里拉平
                Err(e) => Err(e.status())?,
            }
        };
        let size = info.file_size() as usize;

        // 一次性分配内存并读取
        let mut buffer = vec![0u8; size];
        file.read(&mut buffer)?;

        Ok(Self {
            data: buffer,
            cursor: 0,
        })
    }

    /// 模仿之前的 read_frame_next，但改为从内存切片
    pub fn next_frame(&mut self, qoi_buf: &mut Vec<u8>) -> bool {
        if self.cursor >= self.data.len() {
            return false;
        }

        // 假设你的每帧 QOI 数据前面有 4 字节长度信息（这是常见的拼接方式）
        let len_bytes = &self.data[self.cursor..self.cursor + 4];
        let frame_len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;

        self.cursor += 4;
        let end = self.cursor + frame_len;

        // 将这一帧的数据拷贝到 qoi_buf
        qoi_buf.clear();
        qoi_buf.extend_from_slice(&self.data[self.cursor..end]);

        self.cursor = end;
        true
    }

    pub fn rewind(&mut self) {
        self.cursor = 0;
    }
}

//////// 原始数据全缓存
pub struct VideoMemoryRaw {
    // 存储所有帧的像素数据，每一项都是一帧完整的 BltPixel 数组
    pub frames: Vec<Vec<BltPixel>>,
    pub cursor: usize,
}

impl VideoMemoryRaw {
    pub fn new(mut file: RegularFile) -> Result<Self> {
        let mut info_buf = vec![0u8; 128];
        let info = loop {
            match file.get_info::<FileInfo>(&mut info_buf) {
                Ok(info) => break info,
                Err(e) if e.status() == Status::BUFFER_TOO_SMALL => {
                    if let Some(size) = *e.data() { info_buf.resize(size, 0) }
                }
                Err(e) => return Err(e.status())?,
            }
        };

        let mut compressed_buffer = vec![0u8; info.file_size() as usize];
        file.read(&mut compressed_buffer)?;

        // 预解码
        let mut frames = Vec::new();
        let mut offset = 0;

        while offset + 4 <= compressed_buffer.len() {
            // 读取长度信息
            let len_bytes = &compressed_buffer[offset..offset + 4];
            let frame_len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;

            let data_start = offset + 4;
            let next_frame_pos = data_start + frame_len;

            if next_frame_pos > compressed_buffer.len() { break }

            let frame_data = &compressed_buffer[data_start..next_frame_pos];

            // 解码这一帧
            // 我们先解码头来获取分辨率，或者假设你已知分辨率
            // TODO:严重错误 真机上会出现数据错位的情况,概率极大
            let Ok(header) = qoi::decode_header(frame_data) else {
                offset = next_frame_pos;
                continue
            };
            let pixel_count = (header.width * header.height) as usize;

            let mut pixel_buffer = vec![BltPixel::new(0, 0, 0); pixel_count];

            // 完成解码，存入 pixel_buffer
            let Ok(_) = qoi::decode_to_buf(
                // TODO: 4 -> step
                unsafe { core::slice::from_raw_parts_mut(pixel_buffer.as_mut_ptr() as *mut u8, pixel_count * 4) },
                frame_data
            ) else {
                offset = next_frame_pos;
                continue
            };

            for pixel in pixel_buffer.iter_mut() {
                let r = pixel.red;
                pixel.red = pixel.blue;
                pixel.blue = r;
            }

            frames.push(pixel_buffer);
            offset = next_frame_pos;
        }

        Ok(Self {
            frames,
            cursor: 0,
        })
    }

    /// 极致性能：直接返回当前帧的像素引用，完全无拷贝，无解码
    pub fn next_frame(&mut self) -> Option<&[BltPixel]> {
        if self.cursor >= self.frames.len() {
            return None;
        }
        let frame = &self.frames[self.cursor];
        self.cursor += 1;
        Some(frame)
    }

    pub fn rewind(&mut self) {
        self.cursor = 0;
    }
}