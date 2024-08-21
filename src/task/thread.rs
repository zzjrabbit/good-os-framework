use alloc::sync::Arc;
use alloc::sync::Weak;
use core::fmt::Debug;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::RwLock;

use super::context::Context;
use super::process::WeakSharedProcess;
use super::scheduler::add_thread;
use super::scheduler::get_threads;
use super::scheduler::KERNEL_PROCESS;
use super::stack::{KernelStack, UserStack};
use crate::arch::apic::get_lapic_id;
use crate::arch::gdt::Selectors;
use crate::drivers::fpu::FpState;
use crate::memory::KERNEL_PAGE_TABLE;

pub(super) type SharedThread = Arc<RwLock<Thread>>;
pub(super) type WeakSharedThread = Weak<RwLock<Thread>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u64);

impl ThreadId {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Ready,
    Blocked,
    Waiting,
    Terminated,
}

#[allow(dead_code)]
pub struct Thread {
    pub id: ThreadId,
    pub cpu_id: usize,
    pub priority: isize,
    pub vruntime: isize,
    pub state: ThreadState,
    pub kernel_stack: KernelStack,
    pub context: Context,
    pub process: WeakSharedProcess,
    pub fpu_context: FpState,
}

pub const KERNEL_PRIROITY: isize = 10;
pub const USER_PRIROITY: isize = 20;

impl Thread {
    /// Creates a new thread.
    /// Don't call this function directly, use `Thread::new_init_thread`,`Thread::new_user_thread` or `Thread::new_kernel_thread` instead.
    pub fn new(process: WeakSharedProcess, priority: isize) -> Self {
        let thread = Thread {
            id: ThreadId::new(),
            cpu_id: get_lapic_id() as usize,
            priority,
            vruntime: -1,
            state: ThreadState::Ready,
            context: Context::default(),
            kernel_stack: KernelStack::new(),
            process,
            fpu_context: FpState::default(),
        };

        thread
    }

    /// Creates a new initial thread.
    pub fn new_init_thread() -> SharedThread {
        let thread = Self::new(Arc::downgrade(&KERNEL_PROCESS), KERNEL_PRIROITY);
        let thread = Arc::new(RwLock::new(thread));
        thread.write().state = ThreadState::Running;
        thread.write().priority = 1;
        KERNEL_PROCESS.write().threads.push_back(thread.clone());
        add_thread(Arc::downgrade(&thread));

        thread
    }

    /// Creates a new kernel thread.
    pub fn new_kernel_thread(function: fn()) {
        let mut thread = Self::new(Arc::downgrade(&KERNEL_PROCESS), KERNEL_PRIROITY);

        thread.context.init(
            function as usize,
            thread.kernel_stack.end_address(),
            KERNEL_PAGE_TABLE.lock().physical_address,
            Selectors::get_kernel_segments(),
        );

        let thread = Arc::new(RwLock::new(thread));
        add_thread(Arc::downgrade(&thread));
        KERNEL_PROCESS.write().threads.push_back(thread);
    }

    /// Creates a new user thread.
    pub fn new_user_thread(process: WeakSharedProcess, entry_point: usize) {
        let mut thread = Self::new(process.clone(), USER_PRIROITY);
        log::info!("New : {}", thread.id.0);
        let process = process.upgrade().unwrap();
        let mut process = process.write();
        let user_stack = UserStack::new(&mut process.page_table);

        thread.context.init(
            entry_point,
            user_stack.end_address,
            process.page_table.physical_address,
            Selectors::get_user_segments(),
        );

        let thread = Arc::new(RwLock::new(thread));
        add_thread(Arc::downgrade(&thread));
        process.threads.push_back(thread.clone());
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        for (index, thread) in get_threads().iter().enumerate() {
            if let None = thread.upgrade(){
                get_threads().remove(index);
                break;
            }
            if thread.upgrade().unwrap().read().id == self.id {
                get_threads().remove(index);
                break;
            }
        }
    }
}
