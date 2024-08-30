/*
* @file    :   heap.rs
* @time    :   2024/04/14 08:45:00
* @author  :   zzjcarrot
*/

use crate::{memory::FRAME_ALLOCATOR, ref_to_mut, task::Process};
use alloc::sync::Weak;
use core::{alloc::{Allocator, Layout}, ptr::NonNull};
use spin::RwLock;
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags},
    VirtAddr,
};
use talc::*;

pub enum HeapType {
    Kernel,
    User,
}

pub const HEAP_START: u64 = 20 * 1024 * 1024 * 1024 * 1024; // 20TB(用户程序空间18TB~20TB)
pub const USER_HEAP_INIT_SIZE: usize = 128 * 1024; // 128KB

pub struct ProcessHeap {
    heap_type: HeapType,
    size: usize,
    usable_size: usize,
    allocator: Talck<spin::Mutex<()>, ClaimOnOom>,
    process: Option<Weak<RwLock<Process>>>,
}

impl ProcessHeap {
    pub fn new(heap_type: HeapType) -> Self {
        let size = match heap_type {
            HeapType::Kernel => 0,
            HeapType::User => USER_HEAP_INIT_SIZE,
        };
        let allocator = Talck::new(Talc::new(unsafe {
            ClaimOnOom::new(Span::from_base_size(HEAP_START as *mut u8, size))
        }));

        Self {
            heap_type,
            size,
            usable_size: size,
            allocator,
            process: None,
        }
    }

    pub fn init(&self, process: Weak<RwLock<Process>>) {
        match self.heap_type {
            HeapType::User => {
                ref_to_mut(self).process = Some(process.clone());
                let mut frame_allocator = FRAME_ALLOCATOR.lock();
                for page in 0..USER_HEAP_INIT_SIZE / 4096 {
                    let frame = frame_allocator.allocate_frame().unwrap();
                    let page =
                        Page::containing_address(VirtAddr::new(HEAP_START + page as u64 * 4096));
                    let flags = PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE;
                    unsafe {
                        ref_to_mut(&*process.upgrade().unwrap().read())
                            .page_table
                            .map_to(page, frame, flags, &mut *frame_allocator)
                            .unwrap()
                            .flush();
                    }
                }
                
            }

            _ => {}
        }
    }

    fn sbrk(&mut self, size: usize) {
        let page_cnt = (size + 4095) / 4096;
        unsafe {
            let old = Span::from_base_size(HEAP_START as *mut u8, self.size);
            let new = old.extend(0, size);
            self.allocator.lock().extend(old, new);
        };
        let mut frame_allocator = FRAME_ALLOCATOR.lock();
        let process = self.process.as_ref().unwrap().upgrade().unwrap();
        let process = process.read();

        let process = ref_to_mut(&*process);
        for _ in 0..page_cnt {
            let frame = frame_allocator.allocate_frame().unwrap();
            let page = Page::containing_address(VirtAddr::new(HEAP_START + self.size as u64));
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE;
            unsafe {
                process
                    .page_table
                    .map_to(page, frame, flags, &mut *frame_allocator)
                    .unwrap()
                    .flush();
            }

            /*KERNEL_PAGE_TABLE
            .try_get()
            .unwrap()
            .lock()
            .unmap(page)
            .unwrap()
            .1
            .flush();*/

            self.size += 4096;
            self.usable_size += 4096;
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> Option<u64> {
        match self.heap_type {
            HeapType::Kernel => {
                panic!("Don't use process heaps in kernel mode! Use kernel heap instead!")
            }
            _ => {}
        }
        if let Ok(ptr) = self.allocator.allocate(layout) {
            self.usable_size -= layout.size();
            Some(ptr.addr().get() as u64)
        } else {
            self.sbrk(layout.size() * 2);
            let ptr = self.allocator.allocate(layout).unwrap();
            self.usable_size -= layout.size();
            Some(ptr.addr().get() as u64)
        }
    }

    pub fn deallocate(&mut self, ptr: u64, layout: Layout) {
        match self.heap_type {
            HeapType::Kernel => panic!("Don't use process heaps in kernel mode!"),
            _ => {}
        }
        unsafe {
            self.allocator.deallocate(NonNull::new(ptr as *mut u8).unwrap(), layout);
        }
        self.usable_size += layout.size();
    }

    pub fn clear(&mut self) {
        let page_cnt = (self.size + 4095) / 4096;
        let mut frame_allocator = FRAME_ALLOCATOR.lock();
        let process = self.process.as_ref().unwrap().upgrade().unwrap();
        unsafe {
            process.force_write_unlock();
        }
        let mut process = process.write();

        for page in 0..page_cnt {
            let page = Page::containing_address(VirtAddr::new(HEAP_START + page as u64 * 4096));
            let frame = {
                let (frame, mapper_flush) = process
                    .page_table
                    .unmap(page)
                    .unwrap();

                mapper_flush.flush();

                frame
            };
            use x86_64::structures::paging::FrameDeallocator;
            unsafe {
                frame_allocator.deallocate_frame(frame);
            }
        }
        self.size = 0;
        self.usable_size = 0;
    }
}
