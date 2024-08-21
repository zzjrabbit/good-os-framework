use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::*;
use x86_64::structures::paging::page::PageRangeInclusive;
use x86_64::structures::paging::{FrameAllocator, FrameDeallocator};
use x86_64::structures::paging::{Page, Size4KiB};
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame};
use x86_64::{PhysAddr, VirtAddr};

use super::{
    convert_physical_to_virtual, BitmapFrameAllocator, FRAME_ALLOCATOR, PHYSICAL_MEMORY_OFFSET,
};

/// The page table.
#[derive(Debug)]
pub struct GeneralPageTable {
    pub inner: OffsetPageTable<'static>,
    pub physical_address: PhysAddr,
}

impl GeneralPageTable {
    /// Switches to the page table.
    pub unsafe fn switch(&self) {
        let page_table_frame = {
            let physical_address = self.physical_address;
            PhysFrame::containing_address(physical_address)
        };
        if page_table_frame != Cr3::read().0 {
            Cr3::write(page_table_frame, Cr3::read().1);
        }
    }

    /// Creates a new page table from the specified physical address.
    pub unsafe fn new_from_address(
        frame_allocator: &mut BitmapFrameAllocator,
        physical_address: PhysAddr,
    ) -> GeneralPageTable {
        let source_page_table =
            &*convert_physical_to_virtual(physical_address).as_ptr::<PageTable>();
        let mut new_page_table = Self::new(frame_allocator);
        let target_page_table = new_page_table.inner.level_4_table_mut();

        Self::new_from_recursion(frame_allocator, source_page_table, target_page_table, 4);
        new_page_table
    }

    /// Returns the current page table.
    pub unsafe fn ref_from_current() -> Self {
        let physical_address = Cr3::read().0.start_address();

        let page_table =
            &mut *convert_physical_to_virtual(physical_address).as_mut_ptr::<PageTable>();
        let physical_memory_offset = VirtAddr::new(PHYSICAL_MEMORY_OFFSET.clone());
        let offset_page_table = OffsetPageTable::new(page_table, physical_memory_offset);

        Self {
            inner: offset_page_table,
            physical_address,
        }
    }

    /// Creates a new page table.
    unsafe fn new(frame_allocator: &mut BitmapFrameAllocator) -> Self {
        let page_table_address: Option<PhysFrame<Size4KiB>> =
            BitmapFrameAllocator::allocate_frame(frame_allocator);

        let page_table_address = page_table_address
            .expect("Failed to allocate frame for page table!")
            .start_address();

        let new_page_table =
            &mut *convert_physical_to_virtual(page_table_address).as_mut_ptr::<PageTable>();
        let physical_memory_offset = VirtAddr::new(PHYSICAL_MEMORY_OFFSET.clone());
        let page_table = OffsetPageTable::new(new_page_table, physical_memory_offset);

        GeneralPageTable {
            inner: page_table,
            physical_address: page_table_address,
        }
    }

    /// Creates a new page table from the kernel page table.
    unsafe fn new_from_recursion(
        frame_allocator: &mut BitmapFrameAllocator,
        source_page_table: &PageTable,
        target_page_table: &mut PageTable,
        page_table_level: u8,
    ) {
        for (index, entry) in source_page_table.iter().enumerate() {
            if (page_table_level == 1)
                || entry.is_unused()
                || entry.flags().contains(PageTableFlags::HUGE_PAGE)
            {
                target_page_table[index].set_addr(entry.addr(), entry.flags());
                continue;
            }
            let mut new_page_table = Self::new(frame_allocator);
            let new_page_table_address = new_page_table.physical_address;
            target_page_table[index].set_addr(new_page_table_address, entry.flags());

            let source_page_table_next = &*convert_physical_to_virtual(entry.addr()).as_ptr();
            let target_page_table_next = new_page_table.inner.level_4_table_mut();

            Self::new_from_recursion(
                frame_allocator,
                source_page_table_next,
                target_page_table_next,
                page_table_level - 1,
            );
        }
    }

    /// Maps a frame to a page with the specified flags.
    pub unsafe fn map_to_with_table_flags_general(
        &mut self,
        page: Page<Size4KiB>,
        frame: PhysFrame<Size4KiB>,
        flags: PageTableFlags,
        parent_table_flags: PageTableFlags,
    ) {
        let result = self.map_to_with_table_flags(
            page,
            frame,
            flags,
            parent_table_flags,
            &mut *FRAME_ALLOCATOR.lock(),
        );

        match result {
            Ok(flusher) => flusher.flush(),
            Err(err) => match err {
                MapToError::ParentEntryHugePage => {}
                MapToError::PageAlreadyMapped(_) => {
                    self.inner.unmap(page).expect("Cannot unmap to").1.flush();
                    self.inner
                        .map_to_with_table_flags(
                            page,
                            frame,
                            flags,
                            parent_table_flags,
                            &mut *FRAME_ALLOCATOR.lock(),
                        )
                        .unwrap()
                        .flush()
                }
                MapToError::FrameAllocationFailed => panic!("Out of memory"),
            },
        }
    }
}

impl Mapper<Size4KiB> for GeneralPageTable {
    /// Maps the frame to the page with the specified flags.
    #[inline]
    unsafe fn map_to_with_table_flags<A>(
        &mut self,
        page: Page<Size4KiB>,
        frame: PhysFrame<Size4KiB>,
        flags: PageTableFlags,
        parent_table_flags: PageTableFlags,
        allocator: &mut A,
    ) -> Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>>
    where
        A: FrameAllocator<Size4KiB> + ?Sized,
    {
        unsafe {
            self.inner
                .map_to_with_table_flags(page, frame, flags, parent_table_flags, allocator)
        }
    }

    /// unmaps a page.
    #[inline]
    fn unmap(
        &mut self,
        page: Page<Size4KiB>,
    ) -> Result<(PhysFrame<Size4KiB>, MapperFlush<Size4KiB>), UnmapError> {
        self.inner.unmap(page)
    }

    /// updates the flags of the page table.
    #[inline]
    unsafe fn update_flags(
        &mut self,
        page: Page<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<MapperFlush<Size4KiB>, FlagUpdateError> {
        self.inner.update_flags(page, flags)
    }

    /// set the flags of the p4 entry.
    #[inline]
    unsafe fn set_flags_p4_entry(
        &mut self,
        page: Page<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<MapperFlushAll, FlagUpdateError> {
        self.inner.set_flags_p4_entry(page, flags)
    }

    /// sets the flags of the p3 entry.
    #[inline]
    unsafe fn set_flags_p3_entry(
        &mut self,
        page: Page<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<MapperFlushAll, FlagUpdateError> {
        self.inner.set_flags_p3_entry(page, flags)
    }

    /// sets the flags of the p2 entry.
    #[inline]
    unsafe fn set_flags_p2_entry(
        &mut self,
        page: Page<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<MapperFlushAll, FlagUpdateError> {
        self.inner.set_flags_p2_entry(page, flags)
    }

    /// translate a page to a physical frame.
    #[inline]
    fn translate_page(&self, page: Page<Size4KiB>) -> Result<PhysFrame<Size4KiB>, TranslateError> {
        self.inner.translate_page(page)
    }
}

impl Translate for GeneralPageTable {
    /// translate a virtual page to a physical address.
    #[inline]
    fn translate(&self, addr: VirtAddr) -> TranslateResult {
        self.inner.translate(addr)
    }
}

impl CleanUp for GeneralPageTable {
    /// Unmap all the pages in the page table.
    #[inline]
    unsafe fn clean_up<D>(&mut self, frame_deallocator: &mut D)
    where
        D: FrameDeallocator<Size4KiB>,
    {
        self.inner.clean_up(frame_deallocator)
    }

    /// Unmap all the pages in the virtual address range given.
    #[inline]
    unsafe fn clean_up_addr_range<D>(
        &mut self,
        range: PageRangeInclusive,
        frame_deallocator: &mut D,
    ) where
        D: FrameDeallocator<Size4KiB>,
    {
        self.inner.clean_up_addr_range(range, frame_deallocator)
    }
}

impl GeneralPageTable {
    /// Read data from the virtual address on the page table.
    pub fn read(&self, address: VirtAddr, len: usize, buffer: &mut [u8]) -> Result<(), ()> {
        for offset in 0..len {
            let src_address = address + offset as u64;

            let physical_address = self.translate_addr(src_address).ok_or(())?;

            let virtual_address = convert_physical_to_virtual(physical_address);

            let reffer = virtual_address.as_u64() as *const u8;
            buffer[offset] = unsafe { reffer.read() };
        }

        Ok(())
    }

    /// Write data to the virtual address on the page table.
    pub fn write(&self, buffer: &[u8], address: VirtAddr) -> Result<(), ()> {
        for (offset, &byte) in buffer.iter().enumerate() {
            let target_address = address + offset as u64;
            let physical_address = self.translate_addr(target_address).ok_or(())?;
            let virtual_address = convert_physical_to_virtual(physical_address);
            unsafe {
                (virtual_address.as_u64() as *mut u8).write(byte);
            }
        }
        Ok(())
    }
}

/// In syscall, we don't need to worry about page tables, because we are using the user page table.
/// Use this function instead of `write` in syscall.
pub fn write_for_syscall<T: Clone>(addr: VirtAddr, buf: &[T]) {
    let reffer: *mut T = addr.as_mut_ptr();
    for (idx, byte) in buf.iter().enumerate() {
        unsafe {
            reffer.add(idx).write(byte.clone());
        }
    }
}
