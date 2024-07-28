use alloc::collections::btree_map::BTreeMap;
use alloc::format;
use spin::Lazy;
use spin::Mutex;
use x86_64::instructions::port::PortReadOnly;
use x86_64::registers::control::Cr2;
use x86_64::set_general_handler;
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::structures::idt::InterruptStackFrame;
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::VirtAddr;

use super::gdt::DOUBLE_FAULT_IST_INDEX;
use crate::arch::apic::get_lapic_id;
use crate::task::scheduler::SCHEDULERS;

const INTERRUPT_INDEX_OFFSET: u8 = 32;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = INTERRUPT_INDEX_OFFSET,
    ApicError,
    ApicSpurious,
    Keyboard,
    Mouse,
}

pub static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    idt.breakpoint.set_handler_fn(breakpoint);
    idt.segment_not_present.set_handler_fn(segment_not_present);
    idt.invalid_opcode.set_handler_fn(invalid_opcode);
    idt.page_fault.set_handler_fn(page_fault);
    idt.general_protection_fault
        .set_handler_fn(general_protection_fault);

    set_general_handler!(&mut idt, do_irq, 0..255);

    irq_manager_init();
    irq_register(InterruptIndex::Timer as u8, timer_interrupt);
    irq_register(InterruptIndex::ApicError as u8, lapic_error);
    irq_register(InterruptIndex::ApicSpurious as u8, spurious_interrupt);
    irq_register(InterruptIndex::Keyboard as u8, keyboard_interrupt);
    irq_register(InterruptIndex::Mouse as u8, mouse_interrupt);

    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault)
            .set_stack_index(DOUBLE_FAULT_IST_INDEX as u16);
    }

    return idt;
});

fn timer_interrupt(_irq: u8, _error: Option<u64>, _stack: InterruptStackFrame) {
    fn timer_handler(context: VirtAddr) -> VirtAddr {
        let mut schedulers = SCHEDULERS.lock();
        let current_cpu_id = get_lapic_id();
        let scheduler = schedulers
            .get_mut(&current_cpu_id)
            .expect(&format!("Failed to find Processor {}!", current_cpu_id));

        let address = scheduler.schedule(context);

        address
    }

    unsafe {
        core::arch::asm!(
            "cli",
            crate::push_context!(),
            "mov rdi, rsp",
            "call {timer_handler}",
            "mov rsp, rax",
            crate::pop_context!(),
            "sti",
            "iretq",
            timer_handler = sym timer_handler,
            options(noreturn)
        );
    }
}

fn lapic_error(_irq: u8, _error: Option<u64>, _stack: InterruptStackFrame) {
    log::error!("Local APIC error!");
}

fn spurious_interrupt(_irq: u8, _error: Option<u64>, _stack: InterruptStackFrame) {
    log::debug!("Received spurious interrupt!");
}

extern "x86-interrupt" fn segment_not_present(frame: InterruptStackFrame, error_code: u64) {
    log::error!("Exception: Segment Not Present\n{:#?}", frame);
    log::error!("Error Code: {:#x}", error_code);
    panic!("Unrecoverable fault occured, halting!");
}

extern "x86-interrupt" fn general_protection_fault(frame: InterruptStackFrame, error_code: u64) {
    //log::error!("Processor: {}", get_lapic_id());
    log::error!("Exception: General Protection Fault\n{:#?}", frame);
    log::error!("Error Code: {:#x}", error_code);
    x86_64::instructions::hlt();
}

extern "x86-interrupt" fn invalid_opcode(frame: InterruptStackFrame) {
    log::error!("Exception: Invalid Opcode\n{:#?}", frame);
    x86_64::instructions::hlt();
}

extern "x86-interrupt" fn breakpoint(frame: InterruptStackFrame) {
    log::debug!("Exception: Breakpoint\n{:#?}", frame);
}

extern "x86-interrupt" fn double_fault(frame: InterruptStackFrame, error_code: u64) -> ! {
    log::error!("Exception: Double Fault\n{:#?}", frame);
    log::error!("Error Code: {:#x}", error_code);
    panic!("Unrecoverable fault occured, halting!");
}

fn keyboard_interrupt(_irq: u8, _error: Option<u64>, _stack: InterruptStackFrame) {
    let scancode: u8 = unsafe { PortReadOnly::new(0x60).read() };
    crate::drivers::keyboard::add_scancode(scancode);
}

fn mouse_interrupt(_irq: u8, _error: Option<u64>, _stack: InterruptStackFrame) {
    let packet = unsafe { PortReadOnly::new(0x60).read() };
    crate::drivers::mouse::MOUSE.lock().process_packet(packet);
}

extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, error_code: PageFaultErrorCode) {
    log::warn!("Processor: {}", get_lapic_id());
    log::warn!("Exception: Page Fault\n{:#?}", frame);
    log::warn!("Error Code: {:#x}", error_code);
    match Cr2::read() {
        Ok(address) => {
            log::warn!("Fault Address: {:#x}", address);
        }
        Err(error) => {
            log::warn!("Invalid virtual address: {:?}", error);
        }
    }
    x86_64::instructions::hlt();
}

pub fn irq_default_handler(irq: u8, _error: Option<u64>, stack: InterruptStackFrame) {
    log::warn!("default irq: irq = {:#x}, stack = {:?}", irq, stack);
}

static INTERRUPTS_TABLE: Mutex<
    BTreeMap<u8, fn(irq: u8, error: Option<u64>, stack: InterruptStackFrame)>,
> = Mutex::new(BTreeMap::new());

fn irq_manager_init() {
    for i in 0..255 {
        irq_register(i, irq_default_handler);
    }
}

fn do_irq(stack: InterruptStackFrame, irq: u8, error_code: Option<u64>) {
    let table = INTERRUPTS_TABLE.lock();
    let handler = table.get(&irq).expect(&format!("Cannot get irq {}", irq));

    handler(irq, error_code, stack);

    super::apic::end_of_interrupt();
}

pub fn irq_register(irq: u8, handler: fn(irq: u8, error: Option<u64>, stack: InterruptStackFrame)) {
    INTERRUPTS_TABLE.lock().insert(irq, handler);
}

pub fn irq_unregister(_irq: u8) {
    todo!()
}
