use alloc::sync::Arc;
use alloc::sync::Weak;
use core::fmt::Debug;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::RwLock;

use super::context::Context;
use super::process::WeakSharedProcess;
use super::process::KERNEL_PROCESS;
use super::scheduler::SCHEDULER;
use super::stack::{KernelStack, UserStack};
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

impl ThreadState {
    pub fn is_active(&self) -> bool {
        match self {
            ThreadState::Running | ThreadState::Ready => true,
            _ => false,
        }
    }
}

#[allow(dead_code)]
pub struct Thread {
    pub id: ThreadId,
    pub state: ThreadState,
    pub kernel_stack: KernelStack,
    pub context: Context,
    pub process: WeakSharedProcess,
    pub fpu_context: FpState,
}

impl Thread {
    /// Creates a new thread.
    /// Don't call this function directly, use `Thread::new_init_thread`,`Thread::new_user_thread` or `Thread::new_kernel_thread` instead.
    pub fn new(process: WeakSharedProcess) -> Self {
        let thread = Thread {
            id: ThreadId::new(),
            state: ThreadState::Ready,
            context: Context::default(),
            kernel_stack: KernelStack::new(),
            process,
            fpu_context: FpState::default(),
        };

        thread
    }

    /// Creates a new initial thread.
    pub fn get_init_thread() -> WeakSharedThread {
        let thread = Self::new(Arc::downgrade(&KERNEL_PROCESS));
        let thread = Arc::new(RwLock::new(thread));
        KERNEL_PROCESS.write().threads.push_back(thread.clone());
        //SCHEDULER.lock().add(Arc::downgrade(&thread));
        Arc::downgrade(&thread)
    }


    /// Creates a new kernel thread.
    pub fn new_kernel_thread(function: fn()) {
        let mut thread = Self::new(Arc::downgrade(&KERNEL_PROCESS));

        thread.context.init(
            function as usize,
            thread.kernel_stack.end_address(),
            KERNEL_PAGE_TABLE.lock().physical_address,
            Selectors::get_kernel_segments(),
        );

        

        let thread = Arc::new(RwLock::new(thread));
        
        
        SCHEDULER.lock().add(Arc::downgrade(&thread));
        KERNEL_PROCESS.write().threads.push_back(thread);
        
    }

    /// Creates a new user thread.
    pub fn new_user_thread(process: WeakSharedProcess, entry_point: usize) {
        let mut thread = Self::new(process.clone());
        //log::info!("New : {}", thread.id.0);
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
        SCHEDULER.lock().add(Arc::downgrade(&thread));
        process.threads.push_back(thread.clone());
    }
}


