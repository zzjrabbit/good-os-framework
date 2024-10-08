use core::sync::atomic::{AtomicBool, Ordering};

use alloc::collections::vec_deque::VecDeque;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::{Lazy, Mutex};
use x86_64::VirtAddr;

use super::context::Context;
use super::thread::{ThreadState, WeakSharedThread};
use super::Thread;
use crate::arch::apic::get_lapic_id;
use crate::arch::smp::CPUS;

pub static SCHEDULER_INIT: AtomicBool = AtomicBool::new(false);
pub static SCHEDULER: Lazy<Mutex<Scheduler>> = Lazy::new(|| Mutex::new(Scheduler::new()));

pub fn init() {
    SCHEDULER_INIT.store(true, Ordering::SeqCst);
}

pub struct Scheduler {
    current_threads: BTreeMap<u32, WeakSharedThread>,
    ready_threads: VecDeque<WeakSharedThread>,
}

impl Scheduler {
    pub fn new() -> Self {
        let current_threads = CPUS
            .read()
            .iter_id()
            .map(|lapic_id| (*lapic_id, Thread::get_init_thread()))
            .collect();

        Self {
            current_threads,
            ready_threads: VecDeque::new(),
        }
    }

    #[inline]
    pub fn add(&mut self, thread: WeakSharedThread) {
        self.ready_threads.push_front(thread);
    }

    #[inline]
    pub fn remove(&mut self, thread: WeakSharedThread) {
        self.ready_threads.retain(|other| {
            let other = other.upgrade().unwrap();
            !Arc::ptr_eq(&other, &thread.upgrade().unwrap())
        });
    }

    #[inline]
    pub fn current_thread(&self) -> WeakSharedThread {
        let lapic_id = get_lapic_id();
        self.current_threads[&lapic_id].clone()
    }

    pub fn schedule(&mut self, context: VirtAddr) -> VirtAddr {
        let lapic_id = get_lapic_id();

        let last_thread = self.current_threads[&lapic_id]
            .upgrade()
            .and_then(|thread| {
                thread.write().context = Context::from_address(context);
                Some(self.current_threads[&lapic_id].clone())
            });

        if let Some(next_thread) = self.ready_threads.pop_front() {
            self.current_threads.insert(lapic_id, next_thread);
            if let Some(last_thread) = last_thread {
                match last_thread.upgrade().unwrap().read().state {
                    ThreadState::Blocked | ThreadState::Terminated | ThreadState::Waiting => {}
                    _ => self.ready_threads.push_back(last_thread),
                }
            }
        }

        let next_thread = self.current_threads[&lapic_id].upgrade().unwrap();
        let next_thread = next_thread.read();

        let kernel_address = next_thread.kernel_stack.end_address();
        CPUS.write().get_mut(lapic_id).set_ring0_rsp(kernel_address);

        next_thread.context.address()
    }
}

