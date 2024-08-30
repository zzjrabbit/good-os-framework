use crate::memory::FRAME_ALLOCATOR;
use x86_64::structures::paging::{FrameDeallocator, PhysFrame};
use x86_64::{PhysAddr, VirtAddr};

pub mod display;
pub mod fpu;
pub mod hpet;
pub mod keyboard;
pub mod mouse;
pub mod nvme;
pub mod pci;
pub mod rtc;
pub mod serial;
pub mod xhci;

pub fn init() {
    hpet::init();
    mouse::init();
}

/// Allocate memory for the DMA drivers, `cnt` is the number of physical memory frames you need.
pub fn alloc_for_dma(cnt: usize) -> (PhysAddr, VirtAddr) {
    let phys = FRAME_ALLOCATOR.lock().allocate_frames(cnt).unwrap();
    let phys = PhysAddr::new(phys);
    let virt = crate::memory::convert_physical_to_virtual(phys);
    (phys, virt)
}

/// deallocates the physical memory.
pub fn dealloc_for_dma(virt_addr: VirtAddr, _cnt: usize) {
    let phys = crate::memory::convert_virtual_to_physical(virt_addr);
    unsafe {
        FRAME_ALLOCATOR
            .lock()
            .deallocate_frame(PhysFrame::containing_address(phys));
    }
}
