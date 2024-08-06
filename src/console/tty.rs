use core::{
    alloc::Layout,
    slice::from_raw_parts_mut,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::{alloc::alloc, sync::Arc, vec::Vec};
use os_terminal::DrawTarget;
use spin::{Mutex, RwLock};
use x86_64::{
    structures::paging::{Page, PageTableFlags},
    VirtAddr,
};

use crate::{
    drivers::display::Display,
    memory::{FRAME_ALLOCATOR, KERNEL_PAGE_TABLE},
};

pub struct TTY {
    buffer: &'static mut [u8],
    width: usize,
    height: usize,
}

impl TTY {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            buffer: unsafe {
                let addr = alloc(Layout::from_size_align(width * height * 4, 4096).unwrap());
                from_raw_parts_mut(addr, width * height * 4)
            },
            width,
            height,
        }
    }

    pub fn write_pixel(&mut self, x: usize, y: usize, pixel: [u8; 4]) {
        let pos = self.width * y + x;
        let pos = pos * 4;
        let [r, g, b, a] = pixel;
        let pixel = [b, g, r, a];
        self.buffer[pos..pos + 4].copy_from_slice(&pixel);
    }

    pub fn read_pixel(&mut self, x: usize, y: usize) -> [u8; 4] {
        let pos = self.width * y + x;
        let pos = pos * 4;
        let [b, g, r, a] = &self.buffer[pos..pos + 4] else {
            unreachable!()
        };
        [*r, *g, *b, *a]
    }

    pub fn buffer(&self) -> (VirtAddr, usize) {
        (VirtAddr::from_ptr(self.buffer.as_ptr()), self.buffer.len())
    }
}

pub static TTYS: Mutex<Vec<Arc<RwLock<TTY>>>> = Mutex::new(Vec::new());
pub static CURRENT_TTY: AtomicUsize = AtomicUsize::new(0);
pub static INIT: AtomicBool = AtomicBool::new(false);

pub fn switch_to(tty: usize) {
    x86_64::instructions::interrupts::disable();
    let init = INIT.load(Ordering::SeqCst);

    let mut kernel_page_table = KERNEL_PAGE_TABLE.lock();
    let mut frame_allocator = FRAME_ALLOCATOR.lock();
    let ttys = TTYS.lock();
    let frame_buffer = Display::new().get_frame_buffer();

    if init {
        let last_tty_id = CURRENT_TTY.load(Ordering::Relaxed);
        let last_tty = ttys[last_tty_id].clone();

        let (buffer_ptr, buffer_len) = last_tty.read().buffer();
        
        for page_cnt in 0..buffer_len/4096 {
            use x86_64::structures::paging::FrameAllocator;
            use x86_64::structures::paging::Mapper;

            let ptr = buffer_ptr + page_cnt as u64 * 4096;

            unsafe {
                let frame = frame_allocator.allocate_frame().unwrap();

                let (_, flush) = kernel_page_table
                    .unmap(Page::containing_address(ptr))
                    .unwrap();
                flush.flush();

                kernel_page_table
                    .map_to(
                        Page::containing_address(ptr),
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        &mut *frame_allocator,
                    )
                    .unwrap()
                    .flush();
            }
        }

        last_tty.write().buffer.copy_from_slice(frame_buffer);
    }

    CURRENT_TTY.store(tty, Ordering::Relaxed);

    let vram = frame_buffer;
    let mut vram_ptr = VirtAddr::from_ptr(vram.as_ptr());

    let tty = ttys[tty].clone();
    let (buffer_ptr, buffer_len) = tty.read().buffer();

    vram.copy_from_slice(tty.read().buffer);

    for ptr in buffer_ptr..buffer_ptr + buffer_len as u64 {
        use x86_64::structures::paging::FrameDeallocator;
        use x86_64::structures::paging::Mapper;

        unsafe {
            let frame = kernel_page_table
                .translate_page(Page::containing_address(vram_ptr))
                .unwrap();
            let (maped_frame, flush) = kernel_page_table
                .unmap(Page::containing_address(ptr))
                .unwrap();

            frame_allocator.deallocate_frame(maped_frame);
            flush.flush();

            kernel_page_table
                .map_to(
                    Page::containing_address(ptr),
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    &mut *frame_allocator,
                )
                .unwrap()
                .flush();
        }
        vram_ptr += 1;
    }

    if init {
        x86_64::instructions::interrupts::enable();
    }
}

pub struct TTYDrawTarget {
    id: usize,
}

impl TTYDrawTarget {
    pub const fn new(id: usize) -> Self {
        Self { id }
    }
}

impl DrawTarget for TTYDrawTarget {
    fn draw_pixel(&mut self, x: usize, y: usize, color: os_terminal::Rgb888) {
        get_tty(self.id)
            .write()
            .write_pixel(x, y, [color.0, color.1, color.2, 0]);
    }

    fn size(&self) -> (usize, usize) {
        let tty = get_tty(self.id);
        let tty = tty.read();
        (tty.width, tty.height)
    }
}

pub fn init() {
    let (width, height) = super::Display::new().size();
    let mut ttys = TTYS.lock();
    for _ in 0..6 {
        ttys.push(Arc::new(RwLock::new(TTY::new(width, height))));
    }
    drop(ttys);
    switch_to(0);
    INIT.store(true, Ordering::SeqCst);
}

pub fn get_tty(id: usize) -> Arc<RwLock<TTY>> {
    return TTYS.lock()[id].clone();
}
