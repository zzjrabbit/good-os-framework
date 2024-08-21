pub mod context;
pub mod process;
pub mod scheduler;
pub mod signal;
pub mod stack;
pub mod thread;

pub use process::Process;
pub use scheduler::init;
pub use thread::Thread;

/// Schedules the next task.
/// It uses a interrupt.
pub fn schedule() {
    unsafe {
        //log::info!("GO");
        core::arch::asm!("int 0x20");
    }
}
