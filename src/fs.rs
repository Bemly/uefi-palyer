use alloc::vec;
use alloc::vec::Vec;
use uefi::boot::{get_image_file_system, image_handle};
use uefi::CStr16;
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode};

pub fn read_file(path: &CStr16) -> Vec<u8> {
    let mut root_dir = get_image_file_system(image_handle())
        .expect("Failed to get FS")
        .open_volume()
        .expect("Failed to open volume");

    let mut file = root_dir
        .open(path, FileMode::Read, FileAttribute::empty())
        .expect("Failed to open file")
        .into_regular_file()
        .expect("Not a regular file");

    let mut info_buf = [0u8; 128];
    let info = file.get_info::<FileInfo>(&mut info_buf).expect("Failed to get info");

    let mut file_content = vec![0u8; info.file_size() as usize];
    file.read(&mut file_content).expect("Read failed");

    file_content
}