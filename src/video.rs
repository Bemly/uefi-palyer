use uefi::boot::set_watchdog_timer;
use uefi::{cstr16, CStr16};
use uefi::proto::console::gop::BltPixel;
use uefi::proto::media::file::RegularFile;
use crate::fs::Fs;
use crate::graphics::Screen;
use crate::video::buffer::{BltFrameBuffer, QoiFrameBuffer, RawFrameBuffer};
use crate::video::decoder::{VideoMemory, VideoMemoryRaw};
use crate::error::Result;

pub mod buffer;
pub mod decoder;
pub mod ascii_font;


pub fn video_run(screen: &mut Screen) -> Result {

    // 关闭看门狗，如果不行之后写定时喂狗
    set_watchdog_timer(0, 0, None)?;

    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    // const WIDTH: usize = 1280;
    // const HEIGHT: usize = 720;
    const SIZE: usize = WIDTH * HEIGHT;

    let mut fs = Fs::new()?;
    let mut file = fs.open_file(cstr16!("1080p\\video.qois"))?;
    // let mut qoi = QoiFrameBuffer::new(SIZE);
    // let mut raw = RawFrameBuffer::new(SIZE);
    // let mut blt = BltFrameBuffer::new(SIZE);
    // let mut video = VideoMemory::new(file)?;
    let mut video_raw = VideoMemoryRaw::new(file)?;
    screen.draw_u64_optimized_loop(&mut video_raw, WIDTH, HEIGHT); // UNSAFE!!

    loop {
        // draw(&mut fs, &mut file, screen, &mut qoi, &mut raw, &mut blt)?;
        // draw_all_mem(&mut video, screen, &mut qoi, &mut raw, &mut blt)?;
        // draw_all_mem_zero_copy(&mut video, screen, &mut qoi, &mut blt)?;  // UNSAFE!!
        // screen.draw_all_mem_raw_zero_copy(&mut video_raw, WIDTH, HEIGHT); // UNSAFE!!
        // screen.draw_fast_direct_copy(&mut video_raw, WIDTH, HEIGHT);      // UNSAFE!!
        // boot::stall(Duration::from_millis(1000 / 90));
    }

    // Ok(())
}

fn draw_once(fs: &mut Fs, screen: &mut Screen, path: &CStr16, blt_buf: &mut [BltPixel]) -> Result {
    // 加载文件
    let qoi_data = fs.read_file(path)?;

    // 解码图像
    let (header, rgba_buf) = qoi::decode_to_vec(&qoi_data)?;

    // 转换像素格式
    blt_buf.iter_mut()
        .zip(rgba_buf.chunks_exact(header.channels.as_u8() as usize))
        .for_each(|(dest, src)| {
            *dest = BltPixel::new(src[0], src[1], src[2]);
        });

    // 渲染
    screen.draw_image(header.width, header.height, &blt_buf)?;

    Ok(())
}

fn draw_all_mem(
    video: &mut VideoMemory,
    screen: &mut Screen,
    qoi: &mut QoiFrameBuffer,
    raw: &mut RawFrameBuffer,
    blt: &mut BltFrameBuffer
) -> Result {
    if video.next_frame(&mut qoi.0) {
        raw.header = loop {
            match qoi::decode_to_buf(&mut raw.pixels, &qoi.0) {
                Ok(header) => break header,
                Err(qoi::Error::OutputBufferTooSmall { required, .. }) => {
                    raw.pixels.resize(required, 0)
                }
                // TODO:严重错误 真机上会出现数据错位的情况,概率极大
                Err(e) => return Ok(()),
            }
        };

        let pixel_count = raw.get_size();
        let step = raw.header.channels.as_u8() as usize;

        // 这里的 zip 配合 iter_mut 是目前最稳妥的写法
        raw.pixels[..pixel_count * step]
            .chunks_exact(step)
            .zip(blt.0[..pixel_count].iter_mut())
            .for_each(|(r, b)| {
                b.red = r[0];
                b.green = r[1];
                b.blue = r[2];
            });

        // 4. 显示
        screen.draw_image(raw.header.width, raw.header.height, &blt.0)?;
    } else {
        // 读完了，重置指针实现循环播放
        video.rewind();
    }

    Ok(())
}

// 目前只支持4通道互转
fn draw_all_mem_zero_copy(
    video: &mut VideoMemory,
    screen: &mut Screen,
    qoi: &mut QoiFrameBuffer,
    blt: &mut BltFrameBuffer
) -> Result {
    if video.next_frame(&mut qoi.0) {
        // TODO:严重错误 真机上会出现数据错位的情况,概率极大
        let Ok(header) = qoi::decode_header(&qoi.0) else { return Ok(()) };
        let pixel_count = (header.width * header.height) as usize;

        if blt.0.len() < pixel_count {
            blt.0.resize(pixel_count, BltPixel::new(0,0,0));
        }

        // 直接解码到 [R, G, B, A, R, G, B, A...]
        // TODO:严重错误 真机上会出现数据错位的情况,概率极大
        let Ok(_) = qoi::decode_to_buf(as_u8_slice_mut(&mut blt.0[..pixel_count]), &qoi.0)
        else { return Ok(()) };

        // 交换 R 和 B
        for pixel in blt.0[..pixel_count].iter_mut() {
            core::mem::swap(&mut pixel.red, &mut pixel.blue);
        }

        screen.draw_image(header.width, header.height, &blt.0)?;
    } else {
        video.rewind();
    }
    Ok(())
}

fn as_u8_slice_mut(slice: &mut [BltPixel]) -> &mut [u8] {
    let len = slice.len() * core::mem::size_of::<BltPixel>();
    unsafe {
        // 起始地址（指针转换） 总字节长度
        core::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, len)
    }
}

fn draw(
    fs: &mut Fs,
    file: &mut RegularFile,
    screen: &mut Screen,
    qoi: &mut QoiFrameBuffer,
    raw: &mut RawFrameBuffer,
    blt: &mut BltFrameBuffer
) -> Result {
    // 读文件流
    if fs.read_frame_next(file, &mut qoi.0)? {
        // 解码
        raw.header = loop {
            match qoi::decode_to_buf(&mut raw.pixels, &qoi.0) {
                Ok(header) => break header,
                Err(qoi::Error::OutputBufferTooSmall { required, .. }) => {
                    raw.pixels.resize(required, 0)
                }
                // TODO:严重错误 真机上会出现数据错位的情况,概率极大
                // qoi::Error::InvalidPadding
                // qemu无问题 不知道怎么解决 暂时丢帧处理
                Err(e) => return Ok(()),
            }
        };

        // 转换
        // 只取当前帧需要的切片范围，避免处理旧数据
        let step = raw.header.channels.as_u8() as usize;
        let raw_data = raw.pixels[..raw.get_size() * step].chunks_exact(step);
        let blt_data = &mut blt.0[..raw.get_size()];
        for (r, b) in raw_data.zip(blt_data.iter_mut()) {
            b.red = r[0];
            b.green = r[1];
            b.blue = r[2];
        }

        screen.draw_image(raw.header.width, raw.header.height, &blt.0)?;
    } else {
        // 从头读
        file.set_position(0)?;
    }

    Ok(())
}