use alloc::vec;
use alloc::vec::Vec;
use uefi::boot::{get_image_file_system, image_handle};
use uefi::CStr16;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode};
use crate::error::{NyaStatus, Result};


pub struct Fs {
    pub root_dir: Directory
}

impl Fs {
    pub fn new() -> Result<Self> {
        Ok(Self {
            root_dir: get_image_file_system(image_handle())?.open_volume()?
        })
    }

    pub fn read_file(&mut self, path: &CStr16) -> Result<Vec<u8>> {
        let mut file = self.root_dir
            .open(path, FileMode::Read, FileAttribute::empty())?
            .into_regular_file()
            .ok_or(NyaStatus::NotRegularFile)?;

        let mut info_buf = [0u8; 128];
        // todo: MATCH more type
        let info = file.get_info::<FileInfo>(&mut info_buf).expect("Get info failed");

        let mut file_content = vec![0u8; info.file_size() as usize];
        file.read(&mut file_content)?;

        Ok(file_content)
    }
}

