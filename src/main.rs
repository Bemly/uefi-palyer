// #![no_main]
// #![no_std]
//
// extern crate alloc;
//
// mod fs;
// mod graphics;
// mod error;
// mod video;
//
// use uefi::prelude::*;
// use uefi::proto::console::gop::BltPixel;
// use core::time::Duration;
// use uefi::{println, CStr16};
// use uefi::proto::media::file::RegularFile;
// use crate::error::{handle_fatal, Result};
// use crate::fs::Fs;
// use crate::graphics::Screen;
// use crate::video::buffer::{BltFrameBuffer, QoiFrameBuffer, RawFrameBuffer, VideoMemory};
//
// #[entry]
// fn main() -> Status {
//
//     let mut screen = Screen::new().expect("Failed to create screen");
//     run(&mut screen).unwrap_or_else(|e| handle_fatal(e, screen));
//
//     boot::stall(Duration::from_secs(10));
//     Status::SUCCESS
// }
//
// fn run(screen: &mut Screen) -> Result {
//
//     let mut fs = Fs::new()?;
//     let mut file = fs.open_file(cstr16!("anime\\video.qois"))?;
//     let mut qoi = QoiFrameBuffer::new(1280 * 720);
//     let mut raw = RawFrameBuffer::new(1280 * 720);
//     let mut blt = BltFrameBuffer::new(1280 * 720);
//     let mut video = VideoMemory::new(file)?;
//     loop {
//         // draw(&mut fs, &mut file, &mut screen, &mut qoi, &mut raw, &mut blt)?;
//         // draw_all_mem(&mut video, &mut screen, &mut qoi, &mut raw, &mut blt)?;
//         draw_all_mem_zero_copy(&mut video, screen, &mut qoi, &mut blt)?;
//         boot::stall(Duration::from_millis(1000 / 90));
//     }s
//
//
//     // Ok(())
// }
//
// fn draw_once(fs: &mut Fs, screen: &mut Screen, path: &CStr16, blt_buf: &mut [BltPixel]) -> Result {
//     // 加载文件
//     let qoi_data = fs.read_file(path)?;
//
//     // 解码图像
//     let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)?;
//
//     // 转换像素格式
//     blt_buf.iter_mut()
//         .zip(rgba_buf.chunks_exact(header.channels.as_u8() as usize))
//         .for_each(|(dest, src)| {
//             *dest = BltPixel::new(src[0], src[1], src[2]);
//         });
//
//     // 渲染
//     screen.draw_image(header.width, header.height, &blt_buf)?;
//
//     Ok(())
// }
//
// fn draw_all_mem(
//     video: &mut VideoMemory,
//     screen: &mut Screen,
//     qoi: &mut QoiFrameBuffer,
//     raw: &mut RawFrameBuffer,
//     blt: &mut BltFrameBuffer
// ) -> Result {
//     // 1. 从内存中提取下一帧
//     if video.next_frame(&mut qoi.0) {
//         // 2. 解码 (使用你之前的 loop header 逻辑)
//         raw.header = loop {
//             match qoi::decode_to_buf(&mut raw.pixels, &qoi.0) {
//                 Ok(header) => break header,
//                 Err(qoi::Error::OutputBufferTooSmall { required, .. }) => {
//                     raw.pixels.resize(required, 0)
//                 }
//                 Err(e) => Err(e)?, // 假设你实现了转换
//             }
//         };
//
//         // 3. 转换 (优化后的 chunks_exact)
//         let pixel_count = raw.get_size();
//         let step = raw.header.channels.as_u8() as usize;
//
//         // 这里的 zip 配合 iter_mut 是目前最稳妥的写法
//         raw.pixels[..pixel_count * step]
//             .chunks_exact(step)
//             .zip(blt.0[..pixel_count].iter_mut())
//             .for_each(|(r, b)| {
//                 b.red = r[0];
//                 b.green = r[1];
//                 b.blue = r[2];
//             });
//
//         // 4. 显示
//         screen.draw_image(raw.header.width, raw.header.height, &blt.0)?;
//     } else {
//         // 读完了，重置指针实现循环播放
//         video.rewind();
//     }
//
//     Ok(())
// }
//
// // 目前只支持4通道互转
// fn draw_all_mem_zero_copy(
//     video: &mut VideoMemory,
//     screen: &mut Screen,
//     qoi: &mut QoiFrameBuffer,
//     blt: &mut BltFrameBuffer
// ) -> Result {
//     if video.next_frame(&mut qoi.0) {
//         // 1. 预计算所需大小
//         // 注意：这里建议先用 qoi::decode_header 拿宽高，否则 loop 里的 resize 会很难看
//         let header = qoi::decode_header(&qoi.0).map_err(|_| Status::DEVICE_ERROR)?;
//         let pixel_count = (header.width * header.height) as usize;
//
//         if blt.0.len() < pixel_count {
//             blt.0.resize(pixel_count, BltPixel::new(0,0,0));
//         }
//
//         // 2. 直接解码到 BltBuffer 的内存空间
//         // 此时 blt.0 里的数据布局是 [R, G, B, A, R, G, B, A...] (QOI的标准输出)
//         qoi::decode_to_buf(as_u8_slice_mut(&mut blt.0[..pixel_count]), &qoi.0)
//             .map_err(|_| Status::DEVICE_ERROR)?;
//
//         // 3. 原地 (In-place) 交换 R 和 B
//         // 这样不需要 RawBuffer，直接在同一个内存块里把 R 和 B 换位置
//         for pixel in blt.0[..pixel_count].iter_mut() {
//             // 假设 BltPixel 内存布局是 [B, G, R, A]
//             // 解码器填入的是 [R, G, B, A]
//             // 所以我们需要交换 red 字段和 blue 字段的值
//             core::mem::swap(&mut pixel.red, &mut pixel.blue);
//         }
//
//         // 4. 显示
//         screen.draw_image(header.width, header.height, &blt.0)?;
//     } else {
//         video.rewind();
//     }
//     Ok(())
// }
//
// fn as_u8_slice_mut(slice: &mut [BltPixel]) -> &mut [u8] {
//     let len = slice.len() * core::mem::size_of::<BltPixel>();
//     unsafe {
//         // 参数 1：起始地址（指针转换）
//         // 参数 2：总字节长度
//         core::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, len)
//     }
// }
//
// fn draw(
//     fs: &mut Fs,
//     file: &mut RegularFile,
//     screen: &mut Screen,
//     qoi: &mut QoiFrameBuffer,
//     raw: &mut RawFrameBuffer,
//     blt: &mut BltFrameBuffer
// ) -> Result {
//     // 读文件流
//     if fs.read_frame_next(file, &mut qoi.0)? {
//         // 解码
//         raw.header = loop {
//             match qoi::decode_to_buf(&mut raw.pixels, &qoi.0) {
//                 Ok(header) => break header,
//                 Err(qoi::Error::OutputBufferTooSmall { required, .. }) => {
//                     raw.pixels.resize(required, 0)
//                 }
//                 Err(e) => Err(e)?,
//             }
//         };
//
//         // 转换
//         // 只取当前帧需要的切片范围，避免处理旧数据
//         let step = raw.header.channels.as_u8() as usize;
//         let raw_data = raw.pixels[..raw.get_size() * step].chunks_exact(step);
//         let blt_data = &mut blt.0[..raw.get_size()];
//         for (r, b) in raw_data.zip(blt_data.iter_mut()) {
//             b.red = r[0];
//             b.green = r[1];
//             b.blue = r[2];
//         }
//
//         screen.draw_image(raw.header.width, raw.header.height, &blt.0)?;
//     } else {
//         // 从头读
//         file.set_position(0)?;
//     }
//
//     Ok(())
// }


#![no_main]
#![no_std]

use core::time::Duration;
use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive};
use uefi::prelude::*;
use uefi::println;
use uefi::proto::console::gop::{BltOp, BltPixel, GraphicsOutput};

#[entry]
fn main() -> Status {
    let handle = get_handle_for_protocol::<GraphicsOutput>().expect("No GOP");
    let mut gop = open_protocol_exclusive::<GraphicsOutput>(handle).expect("Failed to open GOP");

    let info = gop.current_mode_info();
    let (width, height) = info.resolution();

    gop.blt(BltOp::VideoFill {
        // 白色像素
        color: BltPixel::new(255, 0, 255),
        dest: (0, 0),
        dims: (width, height),
    }).expect("Failed to fill screen");
    drop(gop);

    let _ = uefi::helpers::init();
    println!("Hello, World!");


    boot::stall(Duration::from_mins(1));
    Status::SUCCESS
}