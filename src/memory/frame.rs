use limine::memory_map::EntryType;
use limine::response::MemoryMapResponse;
use x86_64::structures::paging::{FrameAllocator, PhysFrame};
use x86_64::structures::paging::{FrameDeallocator, Size4KiB};
use x86_64::PhysAddr;

use crate::data::bitmap::Bitmap;
use crate::memory::convert_physical_to_virtual;
pub struct BitmapFrameAllocator {
    bitmap: Bitmap,
    usable_frames: usize,
    next_frame: usize,
}

impl BitmapFrameAllocator {
    pub fn init(memory_map: &MemoryMapResponse) -> Self {
        let memory_size = memory_map
            .entries()
            .last()
            .map(|region| region.base + region.length)
            .expect("No memory regions found!");

        let bitmap_size = (memory_size / 4096).div_ceil(8) as usize;

        let usable_regions = memory_map
            .entries()
            .iter()
            .filter(|region| region.entry_type == EntryType::USABLE);

        let bitmap_address = usable_regions
            .clone()
            .find(|region| region.length >= bitmap_size as u64)
            .map(|region| region.base)
            .expect("No suitable memory region for bitmap!");

        let bitmap_buffer = unsafe {
            let physical_address = PhysAddr::new(bitmap_address);
            let virtual_address = convert_physical_to_virtual(physical_address).as_u64();
            core::slice::from_raw_parts_mut(virtual_address as *mut u8, bitmap_size)
        };

        let mut bitmap = Bitmap::new(bitmap_buffer);
        let mut usable_frames = 0;
        let mut next_frame = usize::MAX;

        for region in usable_regions {
            let start_page_index = (region.base / 4096) as usize;
            let frame_count = (region.length / 4096) as usize;

            usable_frames += frame_count;
            next_frame = next_frame.min(start_page_index);

            for index in start_page_index..start_page_index + frame_count {
                bitmap.set(index, true);
            }
        }

        let bitmap_frame_start = (bitmap_address / 4096) as usize;
        let bitmap_frame_count = bitmap_size.div_ceil(4096);
        let bitmap_frame_end = bitmap_frame_start + bitmap_frame_count;

        assert!(next_frame <= bitmap_frame_start);
        if next_frame == bitmap_frame_start {
            next_frame = bitmap_frame_end + 1;
        }
        usable_frames -= bitmap_frame_count;
        (bitmap_frame_start..bitmap_frame_end).for_each(|index| bitmap.set(index, false));

        log::info!("Usable memory: {} KiB", usable_frames * 4);

        BitmapFrameAllocator {
            bitmap,
            usable_frames,
            next_frame,
        }
    }

    /// Allocates some frames.
    pub fn allocate_frames(&mut self, cnt: usize) -> Option<u64> {
        //log::info!("allocate_frames cnt: {}", cnt);
        if cnt > self.usable_frames {
            log::error!("no more usable frames");
            return None;
        }

        self.usable_frames -= cnt;

        let mut next = self.next_frame;
        let mut frame_cnt = 0;
        let mut found = false;
        while next < self.bitmap.len() && self.bitmap.get(next) {
            next += 1;
            frame_cnt += 1;
            if frame_cnt == cnt {
                found = true;
                break;
            }
        }
        if found {
            self.next_frame = next;

            let addr = (next - cnt) * 4096;

            for i in next - cnt..next {
                self.bitmap.set(i, false);
            }

            //log::info!("found!");

            return Some(addr as u64);
        }

        loop {
            //log::info!("next: {}", next);
            if next >= self.bitmap.len() {
                self.usable_frames += cnt;
                return None;
            }
            while next < self.bitmap.len() && !self.bitmap.get(next) {
                next += 1;
            }

            let mut frame_cnt = 0;

            found = false;
            while next < self.bitmap.len() && self.bitmap.get(next) {
                next += 1;
                frame_cnt += 1;
                if frame_cnt == cnt {
                    found = true;
                    break;
                }
            }

            if found {
                let addr = (next - cnt) * 4096;

                for i in next - cnt..next {
                    self.bitmap.set(i, false);
                }

                return Some(addr as u64);
            }
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        if self.usable_frames == 0 {
            log::error!("No more usable frames!");
            return None;
        }

        self.usable_frames -= 1;
        self.bitmap.set(self.next_frame, false);

        let address = self.next_frame * 4096;

        self.next_frame = (self.next_frame + 1..self.bitmap.len())
            .find(|&index| self.bitmap.get(index))
            .unwrap_or(self.bitmap.len());

        Some(PhysFrame::containing_address(PhysAddr::new(address as u64)))
    }
}

impl FrameDeallocator<Size4KiB> for BitmapFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let index = frame.start_address().as_u64() / 4096;
        self.bitmap.set(index as usize, true);
    }
}
