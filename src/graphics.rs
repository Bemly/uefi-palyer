use uefi::boot::{get_handle_for_protocol, open_protocol_exclusive, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};

pub struct Screen {
    gop: ScopedProtocol<GraphicsOutput>,
}

impl Screen {
    pub fn new() -> Self {
        let handle = get_handle_for_protocol::<GraphicsOutput>().expect("GOP not found");
        let gop = open_protocol_exclusive::<GraphicsOutput>(handle).expect("GOP open failed");
        Self { gop }
    }

    pub fn draw_image(&mut self, width: u32, height: u32, pixels: &[BltPixel]) {
        self.gop.blt(BltOp::BufferToVideo {
            buffer: pixels,
            src: BltRegion::Full,
            dest: (0, 0),
            dims: (width as usize, height as usize),
        }).expect("Blt failed");
    }
}