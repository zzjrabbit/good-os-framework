use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use core::fmt::Debug;
use core::sync::atomic::{AtomicU64, Ordering};
use object::{File, Object, ObjectSegment};
use spin::RwLock;
use x86_64::instructions::interrupts;
use x86_64::structures::paging::mapper::CleanUp;
use x86_64::structures::paging::PageTableFlags;
use x86_64::VirtAddr;

use super::scheduler::get_process;
use super::signal::SignalManager;
use super::thread::{SharedThread, Thread, ThreadState};
use crate::memory::MemoryManager;
use crate::memory::{create_page_table_from_kernel, HeapType, ProcessHeap};
use crate::memory::{GeneralPageTable, FRAME_ALLOCATOR};
use crate::task::scheduler::add_process;

pub(super) type SharedProcess = Arc<RwLock<Process>>;
pub(super) type WeakSharedProcess = Weak<RwLock<Process>>;

const KERNEL_PROCESS_NAME: &str = "kernel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

impl ProcessId {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        ProcessId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[allow(dead_code)]
pub struct Process {
    pub id: ProcessId,
    name: String,
    pub page_table: GeneralPageTable,
    pub threads: VecDeque<SharedThread>,
    pub heap: ProcessHeap,
    pub signal_manager: SignalManager,
    pub father: Option<WeakSharedProcess>,
}

fn create_wake_up_function(id: ProcessId) {
    let process = get_process(id).unwrap();
    unsafe {
        process.force_write_unlock();
    }
    for thread in process.read().threads.iter() {
        unsafe {
            thread.force_write_unlock();
        }
        thread.write().state = ThreadState::Ready;
        log::info!("waking up thread {}",thread.read().id.0);
    }
    log::info!("Woke up {:?}",id);
}

impl Process {
    /// Creates a new process.
    /// Don't use this function directly, use `new_user_process` instead.
    pub fn new(name: &str, heap_type: HeapType) -> Self {
        let page_table = create_page_table_from_kernel();
        let pid = ProcessId::new();
        let process = Process {
            id: pid,
            name: String::from(name),
            page_table,
            threads: Default::default(),
            heap: ProcessHeap::new(heap_type),
            signal_manager: SignalManager::new(64, create_wake_up_function, pid),
            father: None,
        };

        process
    }

    /// Creates a new kernel process.
    /// Dont't use this function. There should be only one kernel process.
    pub fn new_kernel_process() -> SharedProcess {
        let process = Arc::new(RwLock::new(Self::new(
            KERNEL_PROCESS_NAME,
            HeapType::Kernel,
        )));
        process.read().heap.init(Arc::downgrade(&process));
        process
    }

    /// Creates a new user process.
    pub fn new_user_process(name: &str, elf_data: &'static [u8]) -> SharedProcess {
        let binary = ProcessBinary::parse(elf_data);
        let process = Arc::new(RwLock::new(Self::new(name, HeapType::User)));
        process.read().heap.init(Arc::downgrade(&process));
        ProcessBinary::map_segments(&binary, &mut process.write().page_table);
        log::info!("User Entry Point: {:x} ID: {:?}", binary.entry(),process.read().id);
        Thread::new_user_thread(Arc::downgrade(&process), binary.entry() as usize);
        add_process(process.clone());
        process
    }
}

impl Drop for Process {
    /// drop the data of the process.
    fn drop(&mut self) {
        unsafe { self.page_table.clean_up(&mut *FRAME_ALLOCATOR.lock()) };
    }
}

struct ProcessBinary;

impl ProcessBinary {
    fn parse(bin: &'static [u8]) -> File<'static> {
        File::parse(bin).expect("Failed to parse ELF binary!")
    }

    fn map_segments(elf_file: &File, page_table: &mut GeneralPageTable) {
        interrupts::without_interrupts(|| {
            for segment in elf_file.segments() {
                let segment_address = VirtAddr::new(segment.address() as u64);

                let flags = PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE;

                <MemoryManager>::alloc_range(segment_address, segment.size(), flags, page_table)
                    .expect("Failed to allocate memory for ELF segment!");

                if let Ok(data) = segment.data() {
                    page_table.write(data, segment_address).unwrap();
                }
            }
        });
    }
}
