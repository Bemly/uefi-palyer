use alloc::vec;
use alloc::vec::Vec;
use qoi::Header;
use uefi::proto::console::gop::BltPixel;
use uefi::proto::media::file::{File, FileInfo, RegularFile};
use uefi::Status;
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
        // 1. 获取文件大小
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

        // 2. 一次性分配内存并读取
        let mut buffer = vec![0u8; size];
        file.read(&mut buffer).map_err(|_| Status::DEVICE_ERROR)?;

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