use core::sync::atomic::{AtomicBool, Ordering};

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Lazy, Mutex, RwLock};
use x86_64::instructions::interrupts;
use x86_64::VirtAddr;

use super::context::Context;
use super::process::SharedProcess;
use super::thread::{SharedThread, ThreadState, WeakSharedThread};
use super::{Process, Thread};
use crate::arch::apic::get_lapic_id;
use crate::arch::smp::CPUS;

pub static SCHEDULER_INIT: AtomicBool = AtomicBool::new(false);
pub static SCHEDULERS: Mutex<BTreeMap<u32, Scheduler>> = Mutex::new(BTreeMap::new());
pub static KERNEL_PROCESS: Lazy<SharedProcess> = Lazy::new(|| Process::new_kernel_process());
static PROCESSES: RwLock<Vec<SharedProcess>> = RwLock::new(Vec::new());
static THREADS: Mutex<Vec<WeakSharedThread>> = Mutex::new(Vec::new());

pub fn init() {
    add_process(KERNEL_PROCESS.clone());

    let id = CPUS.lock().bsp_id();

    SCHEDULERS
        .lock()
        .insert(id, Scheduler::new());

    //x86_64::instructions::interrupts::enable();
    SCHEDULER_INIT.store(true, Ordering::Relaxed);
    log::info!("Scheduler initialized!");
}

#[inline]
pub fn add_process(process: SharedProcess) {
    interrupts::without_interrupts(|| {
        PROCESSES.write().push(process.clone());
    });
}

#[inline]
pub fn add_thread(thread: WeakSharedThread) {
    interrupts::without_interrupts(|| {
        let cpu_num = CPUS.lock().cpu_num();

        let mut threads = THREADS.lock();
        let thread = thread.upgrade().unwrap();

        let mut min_loads_cpu_id: usize = thread.read().cpu_id;
        let mut min_loads = threads.len();

        for cpu_id in 0..cpu_num {
            let mut tmp_cpu_loads = 0;
            if threads.len() > 0 {
                for thread in threads.iter() {
                    let thread = thread.upgrade().unwrap();
                    if thread.read().cpu_id != cpu_id {
                        continue;
                    }
                    tmp_cpu_loads += 1;
                }
            } else {
                break;
            }

            if min_loads - tmp_cpu_loads > 0 {
                min_loads_cpu_id = cpu_id;
                min_loads = tmp_cpu_loads;
            }
        }

        if min_loads_cpu_id != thread.read().cpu_id {
            thread.write().cpu_id = min_loads_cpu_id;
        }

        threads.push(Arc::downgrade(&thread));
    });
}

pub struct Scheduler {
    pub current_thread: SharedThread,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            current_thread: Thread::new_init_thread(),
        }
    }

    pub fn get_next(&mut self, cpu_id: usize) -> Option<SharedThread> {
        let mut threads = THREADS.lock();
        let mut idx = 0;
        while idx < threads.len() {
            let thread0 = threads.remove(0);
            let thread = thread0.upgrade().unwrap();
            threads.push(thread0);
            if thread.read().state == ThreadState::Ready && thread.read().cpu_id == cpu_id {
                return Some(thread.clone());
            }
            idx += 1;
        }
        None
    }

    pub fn schedule(&mut self, context: VirtAddr) -> VirtAddr {
        let last_thread = {
            let mut thread = self.current_thread.write();
            thread.context = Context::from_address(context);
            self.current_thread.clone()
        };

        let current_cpu_id = get_lapic_id();
        let next_thread = self.get_next(current_cpu_id as usize);
        if let None = next_thread {
            return context;
        }
        let next_thread = next_thread.unwrap();

        next_thread.write().state = ThreadState::Running;
        self.current_thread = next_thread;

        last_thread.write().state = ThreadState::Ready;

        let next_thread = self.current_thread.read();

        let kernel_address = next_thread.kernel_stack.end_address();
        CPUS.lock().current_cpu().1.set_ring0_rsp(kernel_address);

        next_thread.context.address()
    }
}
