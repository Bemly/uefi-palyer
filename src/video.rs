use alloc::boxed::Box;
use alloc::{format, vec};
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, set_watchdog_timer, EventType, Tpl};
use uefi::{cstr16, println, CStr16, Status};
use uefi::proto::console::gop::{BltPixel, GraphicsOutput};
use uefi::proto::media::file::{File, FileInfo, RegularFile};
use uefi::proto::pi::mp::MpServices;
use crate::fs::Fs;
use crate::graphics::Screen;
use crate::video::buffer::{BltFrameBuffer, QoiFrameBuffer, RawFrameBuffer};
use crate::video::decoder::{VideoMemory, VideoMemoryRaw};
use crate::error::{handle_fatal, NyaStatus, Result};

pub mod buffer;
pub mod decoder;
pub mod ascii_font;


pub fn video_run(screen: &mut Screen) -> Result {

    // 关闭看门狗，如果不行之后写定时喂狗
    set_watchdog_timer(0, 0, None)?;

    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    const SIZE: usize = WIDTH * HEIGHT;

    let mut fs = Fs::new()?;
    let mut file = fs.open_file(cstr16!("1080p\\video.qois"))?;
    // let mut qoi = QoiFrameBuffer::new(SIZE);
    // let mut raw = RawFrameBuffer::new(SIZE);
    // let mut blt = BltFrameBuffer::new(SIZE);
    // let mut video = VideoMemory::new(file)?;
    // let mut video_raw = VideoMemoryRaw::new(file)?;
    // screen.parallel_video_draw_ultra(&mut video_raw, WIDTH, HEIGHT)?;
    // screen.draw_u64_optimized_loop(&mut video_raw, WIDTH, HEIGHT); // SUPER UNSAFE!!
    mp_draw(screen, &mut file, WIDTH, HEIGHT)?;

    // loop {
    //     draw(&mut fs, &mut file, screen, &mut qoi, &mut raw, &mut blt)?;
    //     draw_all_mem(&mut video, screen, &mut qoi, &mut raw, &mut blt)?;
    //     draw_all_mem_zero_copy(&mut video, screen, &mut qoi, &mut blt)?;  // UNSAFE!!
    //     screen.draw_all_mem_raw_zero_copy(&mut video_raw, WIDTH, HEIGHT); // UNSAFE!!
    //     screen.draw_fast_direct_copy(&mut video_raw, WIDTH, HEIGHT);      // UNSAFE!!
    //     boot::stall(Duration::from_millis(1000 / 90));
    // }

    Ok(())
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

type PixelPoint = u8;
type Frame = Vec<PixelPoint>;
type FrameSegment = Vec<Frame>;
type CoreFrameSegment = Vec<FrameSegment>;

#[repr(C)]
struct PlayTask<'a> {
    mp: &'a MpServices, // 用于 who_am_i
    fb_base: *mut u8,
    stride_bytes: usize,
    width: usize,
    height: usize,
    num_cores: usize,
    // core_frames[核心ID][帧ID] -> 这一帧该核负责的像素切片
    // 注意：这里需要是指针的指针，因为 AP 无法直接访问 Vec 的元数据
    core_frame_ptrs: *const *const *const u8,
    total_frames: usize,
    sync_counter: &'a AtomicUsize, // 关键：原子计数器
}

extern "efiapi" fn play_task(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &*(arg as *const PlayTask) };

    let my_id = unsafe { (*ctx.mp).who_am_i().unwrap_or(0) };
    if my_id >= ctx.num_cores { return; }

    // 核心参数计算
    let rows_per_core = ctx.height / ctx.num_cores;
    let y_start = my_id * rows_per_core;
    let my_fb_ptr = unsafe { ctx.fb_base.add(y_start * ctx.stride_bytes) };
    let my_frames_list = unsafe { *ctx.core_frame_ptrs.add(my_id) };
    let my_block_size = if my_id == ctx.num_cores - 1 {
        (ctx.height - y_start) * ctx.stride_bytes
    } else {
        rows_per_core * ctx.stride_bytes
    };

    // 使用我们传入的原子计数器来控制进度
    // 我们定义：frame_idx = sync_counter / num_cores
    loop {
        // 1. 获取当前大家共有的帧索引
        // 这里不能直接 fetch_add，因为每个核都要用同一个值写完这一轮
        let current_count = ctx.sync_counter.load(Ordering::Acquire);
        let frame_idx = (current_count / ctx.num_cores) % ctx.total_frames;

        // 2. 搬运属于自己的那一块数据
        unsafe {
            let src_ptr = *my_frames_list.add(frame_idx);
            core::ptr::copy_nonoverlapping(src_ptr, my_fb_ptr, my_block_size);
        }

        // 3. 同步：完成任务，打卡签到
        // 每个核完成一帧的局部拷贝后，给原子变量 +1
        ctx.sync_counter.fetch_add(1, Ordering::SeqCst);

        // 4. 等待其他核心。只有当所有核心都写完了这一帧，
        // sync_counter 才会达到 (frame_idx + 1) * num_cores
        let target = (current_count / ctx.num_cores + 1) * ctx.num_cores;
        while ctx.sync_counter.load(Ordering::Acquire) < target {
            core::hint::spin_loop(); // 自旋等待“慢队友”
        }

        // 此时，大家一起跳出循环，进入下一轮 loop，重新计算新的 frame_idx
    }
}

pub fn mp_draw(screen: &mut Screen, file: &mut RegularFile, width: usize, height: usize) -> Result {
    // 1 解码
    let mp_handle = get_handle_for_protocol::<MpServices>()?;
    let mp = open_protocol_exclusive::<MpServices>(mp_handle)?;
    let n_cores = mp.get_number_of_processors()?.enabled;
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
    file.read(&mut compressed_buffer).map_err(|e| e.status())?;

    // 准备核心存储容器: [核心ID][帧ID] -> 这一帧该核负责的像素切片
    let mut core_frames: CoreFrameSegment = (0..n_cores).map(|_| Vec::new()).collect();
    let mut offset = 0;

    let (scr_width, scr_height) = screen.get_gop().current_mode_info().resolution();
    let rows_per_core = scr_height / n_cores;

    let fb_base = screen.get_gop().frame_buffer().as_mut_ptr();
    let scr_stride = screen.get_gop().current_mode_info().stride();

    // 原始单帧空间初始化
    let mut single_raw: Frame = vec![0u8; width * height * 4];
    // 文件末尾越界保护
    while offset + 4 <= compressed_buffer.len() {
        let len_bytes = &compressed_buffer[offset..offset + 4];
        let frame_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        let data_start = offset + 4;
        let next_frame_pos = data_start + frame_len;

        if next_frame_pos > compressed_buffer.len() { break }

        // 获取当前Qoi帧
        let frame_data = &compressed_buffer[data_start..next_frame_pos];

        if qoi::decode_to_buf(&mut single_raw, frame_data).is_err() {
            offset = next_frame_pos;
            continue
        }

        // 红蓝交换 (RGBA -> BGRA)
        for chunk in single_raw.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        // 核心切分
        for core_id in 0..n_cores {
            let y_start = core_id * rows_per_core; // 该核心负责的屏幕起始行
            let y_end = if core_id == n_cores - 1 { scr_height } else { y_start + rows_per_core };

            let mut frame: Frame = Vec::new();
            let stride_bytes = scr_stride * 4; // 屏幕物理跨度
            let row_bytes = width * 4;        // 视频实际宽度

            for y in y_start..y_end {
                let video_y = y;

                if video_y < height {
                    // 只有当屏幕行落在视频高度范围内时，才去 single_raw 拿数据
                    let src_row_start = video_y * width * 4;
                    let src_row_end = src_row_start + row_bytes;

                    if src_row_end > single_raw.len() {
                        Err(Status::INVALID_PARAMETER)?
                    }

                    // 1. 拷贝视频行
                    frame.extend_from_slice(&single_raw[src_row_start..src_row_end]);

                    // 2. 补齐当前行到屏幕的 stride 跨度
                    if stride_bytes > row_bytes {
                        let padding = stride_bytes - row_bytes;
                        frame.resize(frame.len() + padding, 0u8);
                    }
                } else {
                    // 如果屏幕行超过了视频高度，这一行全是黑的（但也要占满 stride 长度）
                    frame.resize(frame.len() + stride_bytes, 0u8);
                }
            }
            core_frames[core_id].push(frame);
        }
        offset = next_frame_pos;
    }

    // 2 构造参数
    let stride_bytes = scr_stride * 4;

    // 用来存储所有核心任务包的指针

    // --- 准备指针矩阵 ---
    // 我们需要把 Vec<Vec<Vec<u8>>> 转换成三级指针，方便 AP 访问
    let mut all_core_lists = Vec::new();
    for core_id in 0..n_cores {
        let frame_addrs: Vec<*const u8> = core_frames[core_id].iter().map(|f| f.as_ptr()).collect();
        all_core_lists.push(Box::leak(frame_addrs.into_boxed_slice()).as_ptr());
    }
    let core_frame_ptrs = Box::leak(all_core_lists.into_boxed_slice()).as_ptr();

    let (scr_width, scr_height) = screen.get_gop().current_mode_info().resolution();
    let scr_stride = screen.get_gop().current_mode_info().stride();
    let fb_base = screen.get_gop().frame_buffer().as_mut_ptr();

    let sync_counter = Box::leak(Box::new(AtomicUsize::new(0)));

    // --- 构造统一 Context ---
    let ctx = Box::leak(Box::new(PlayTask {
        mp: &mp,
        fb_base,
        stride_bytes: scr_stride * 4,
        width,
        height: scr_height,
        num_cores: n_cores,
        core_frame_ptrs,
        total_frames: core_frames[0].len(),
        sync_counter
    }));

    let arg_ptr = ctx as *mut _ as *mut c_void;

    let event = unsafe { create_event(EventType::empty(), Tpl::CALLBACK, None, None)? };

    // --- 启动 AP ---
    if n_cores > 1 {
        // 使用你彩色代码中成功的 startup_all_aps
        // 注意：如果你需要它不阻塞持续播放，这里要设置为 false (非阻塞启动)
        let _ = mp.startup_all_aps(false, play_task, arg_ptr, Some(event), None);
    }

    // --- BSP 亲自执行 ---
    play_task(arg_ptr);

    Ok(())
}