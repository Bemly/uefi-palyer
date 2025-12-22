use alloc::vec;
use alloc::vec::Vec;
use uefi::boot::{get_image_file_system, image_handle};
use uefi::{CStr16, Status};
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, RegularFile};
use crate::error::{NyaStatus, Result};


pub struct Fs {
    pub root_dir: Directory,
}

impl Fs {
    pub fn new() -> Result<Self> {
        Ok(Self {
            root_dir: get_image_file_system(image_handle())?.open_volume()?
        })
    }

    // 没有方法重载 没有可选参数 没有默认形参值！草了
    // <- 给古老的我:对于编译时确定的值可以用宏($($arg:tt)*)来实现可变参数
    // <- 给过去的我:骗你的，format_args是硬编码
    #[inline]
    pub fn open_file(&mut self, path: &CStr16) -> Result<RegularFile> {
        self.open_file_mode(path, FileMode::Read)
    }

    #[inline]
    pub fn open_file_mode(&mut self, path: &CStr16, mode: FileMode) -> Result<RegularFile> {
        self.open_file_attr(path, mode, FileAttribute::empty())
    }

    #[inline]
    pub fn open_file_attr(&mut self, path: &CStr16, mode: FileMode, attr: FileAttribute) -> Result<RegularFile> {
        self.root_dir
            .open(path, mode, attr)?
            .into_regular_file()
            .ok_or(NyaStatus::NotRegularFile)
    }

    // 一次性读取全部内容，慎用
    pub fn read_file(&mut self, path: &CStr16) -> Result<Vec<u8>> {
        let mut file = self.open_file(path)?;

        // 获取文件信息
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

        // 获取完整内容
        let mut file_content = vec![0u8; info.file_size() as usize];
        file.read(&mut file_content)?;

        Ok(file_content)
    }

    pub fn read_frame_next(&mut self, file: &mut RegularFile, buf: &mut Vec<u8>) -> Result<bool> {
        // 读头
        let mut qoi_size = [0u8; 4];
        if file.read(&mut qoi_size)? < 4 {
            return Ok(false);
        }
        let qoi_size = u32::from_le_bytes(qoi_size) as usize;

        // 扩容缓冲区
        if buf.len() < qoi_size {
            buf.resize(qoi_size, 0);
        }

        // 写入一帧
        file.read(&mut buf[..qoi_size])?;

        Ok(true)
    }
}

