use frame::BitmapFrameAllocator;
use limine::request::{HhdmRequest, MemoryMapRequest};
use spin::{Lazy, Mutex};
use x86_64::{instructions::interrupts, PhysAddr, VirtAddr};

mod frame;
mod kernel_heap;
mod manager;
mod page_table;
mod user_heap;

pub use kernel_heap::init;
pub use manager::MemoryManager;
pub use page_table::*;
pub use user_heap::*;

#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests"]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

pub static PHYSICAL_MEMORY_OFFSET: Lazy<u64> =
    Lazy::new(|| HHDM_REQUEST.get_response().unwrap().offset());

/// The global Frame Allocator.
pub static FRAME_ALLOCATOR: Lazy<Mutex<BitmapFrameAllocator>> = Lazy::new(|| {
    let memory_map = MEMORY_MAP_REQUEST.get_response().unwrap();
    Mutex::new(BitmapFrameAllocator::init(memory_map))
});

/// The page table that limine prepared for us.
pub static KERNEL_PAGE_TABLE: Lazy<Mutex<GeneralPageTable>> = Lazy::new(|| {
    let page_table = unsafe { GeneralPageTable::ref_from_current() };
    Mutex::new(page_table)
});

/// Convert the physical address to a virtual address.
#[inline]
pub fn convert_physical_to_virtual(physical_address: PhysAddr) -> VirtAddr {
    VirtAddr::new(physical_address.as_u64() + PHYSICAL_MEMORY_OFFSET.clone())
}

/// Convert the virtual address to a physical address.
#[inline]
pub fn convert_virtual_to_physical(virtual_address: VirtAddr) -> PhysAddr {
    PhysAddr::new(virtual_address.as_u64() - PHYSICAL_MEMORY_OFFSET.clone())
}

/// Copies a page table from the kernel.
pub fn create_page_table_from_kernel() -> GeneralPageTable {
    interrupts::without_interrupts(|| {
        let mut frame_allocator = FRAME_ALLOCATOR.lock();
        let page_table_address = KERNEL_PAGE_TABLE.lock().physical_address;
        unsafe { GeneralPageTable::new_from_address(&mut frame_allocator, page_table_address) }
    })
}

/// Read something from the address.
pub fn read_from_addr<T>(addr: VirtAddr) -> T {
    unsafe { addr.as_ptr::<T>().read() }
}

/// Returns a mutable reference to the address.
pub fn addr_to_mut_ref<T>(addr: VirtAddr) -> &'static mut T {
    unsafe { &mut (*addr.as_mut_ptr()) }
}

/// Returns a mutable reference to the array on the address.
pub fn addr_to_array<T>(addr: VirtAddr, len: usize) -> &'static mut [T] {
    unsafe { core::slice::from_raw_parts_mut(addr.as_mut_ptr(), len) }
}
