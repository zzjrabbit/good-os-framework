use core::sync::atomic::{AtomicBool, Ordering};
use core::usize;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Lazy, Mutex, MutexGuard, RwLock};
use x86_64::instructions::interrupts;
use x86_64::VirtAddr;

use super::context::Context;
use super::process::{ProcessId, SharedProcess};
use super::thread::{SharedThread, ThreadState, WeakSharedThread};
use super::{Process, Thread};
use crate::arch::apic::get_lapic_id;
use crate::arch::smp::{BSP_LAPIC_ID, CPUS};
use crate::ref_to_mut;

pub static SCHEDULER_INIT: AtomicBool = AtomicBool::new(false);
pub static SCHEDULERS: Mutex<BTreeMap<u32, Scheduler>> = Mutex::new(BTreeMap::new());
pub static KERNEL_PROCESS: Lazy<SharedProcess> = Lazy::new(|| Process::new_kernel_process());
static PROCESSES: RwLock<BTreeMap<ProcessId, SharedProcess>> = RwLock::new(BTreeMap::new());
static THREADS: Mutex<Vec<WeakSharedThread>> = Mutex::new(Vec::new());

pub fn init() {
    add_process(KERNEL_PROCESS.clone());

    let id = *BSP_LAPIC_ID;

    SCHEDULERS.lock().insert(id, Scheduler::new());

    //x86_64::instructions::interrupts::enable();
    SCHEDULER_INIT.store(true, Ordering::SeqCst);
    log::info!("Scheduler initialized!");
}

/// Gets the process by the process ID given.
pub fn get_process(pid: ProcessId) -> Option<SharedProcess> {
    PROCESSES.read().get(&pid).cloned()
}

/// Adds a new process.
/// You don't need to call this function directly.
/// The `new_user_process` function calls this function.
#[inline]
pub fn add_process(process: SharedProcess) {
    interrupts::without_interrupts(|| {
        PROCESSES.write().insert(process.read().id, process.clone());
    });
}

/// Adds a new thread.
/// You don't need to call this function directly.
/// The `Thread::new_user_thread` and `Thread::new_kernel_thread` function calls this function.
#[inline]
pub fn add_thread(thread: WeakSharedThread) {
    interrupts::without_interrupts(|| {
        let cpu_num = CPUS.lock().len();

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
            if thread.read().state == ThreadState::Ready && thread.read().cpu_id == cpu_id && thread.read().id != self.current_thread.read().id {
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
            thread.fpu_context.save();
            thread.vruntime -= 1;
            self.current_thread.clone()
        };

        if last_thread.read().vruntime <= 0 || last_thread.read().state == ThreadState::Terminated {
            if last_thread.read().state == ThreadState::Terminated {
                log::info!("in");
            }
            let current_cpu_id = get_lapic_id();
            let next_thread = self.get_next(current_cpu_id as usize);
            if let None = next_thread {
                if last_thread.read().state == ThreadState::Terminated {
                    panic!("Could not get the next thread to run! CPU is hungry!");
                }
                last_thread.read().fpu_context.restore();
                return context;
            }
            let next_thread = next_thread.unwrap();

            next_thread.write().state = ThreadState::Running;
            self.current_thread = next_thread;

            if last_thread.read().state == ThreadState::Running {
                let last_thread_priority = last_thread.read().priority;
                last_thread.write().vruntime = last_thread_priority;
                last_thread.write().state = ThreadState::Ready;
            }

            let next_thread = self.current_thread.read();

            //crate::print!("[{} {}]", get_lapic_id(), next_thread.id.0);

            let kernel_address = next_thread.kernel_stack.end_address();
            CPUS.lock().get_mut(get_lapic_id()).set_ring0_rsp(kernel_address);

            next_thread.fpu_context.restore();

            let addr = next_thread.context.address();

            //crate::print!("[{:x} {}]", addr, next_thread.id.0);

            return addr;
        }

        last_thread.read().fpu_context.restore();
        return context;
    }
}

/// DO NOT USE THIS FUNCTION!
pub fn get_threads() -> MutexGuard<'static, Vec<WeakSharedThread>> {
    THREADS.lock()
}

/// exits the current process.
pub fn exit() {
    let schedulers = SCHEDULERS.lock();
    let current_scheduler_option = schedulers.get(&get_lapic_id());

    if let Some(current_scheduler) = current_scheduler_option {
        let current_process = &current_scheduler.current_thread.read().process;
        let current_process = current_process.upgrade().unwrap();

        let mut current_process_write = current_process.write();

        for thread in current_process_write.threads.iter() {
            ref_to_mut(&*thread.read()).state = ThreadState::Terminated
        }

        let pid = current_process_write.id;

        current_process_write.heap.clear();

        drop(current_process_write);
        drop(current_process);

        PROCESSES.write().remove(&pid);
        // log::info!("{}", PROCESSES.read().len());
    } else {
        log::warn!("current thead is None");
    }

    drop(schedulers);
}
