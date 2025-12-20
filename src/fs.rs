use alloc::vec;
use alloc::vec::Vec;
use uefi::boot::{get_image_file_system, image_handle};
use uefi::CStr16;
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode};
use crate::error::{NyaStatus, Result};

pub fn read_file(path: &CStr16) -> Result<Vec<u8>> {
    let mut root_dir = get_image_file_system(image_handle())?
        .open_volume()?;

    let mut file = root_dir
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