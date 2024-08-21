use conquer_once::spin::OnceCell;
use spin::Mutex;
use x2apic::ioapic::{IoApic, IrqMode, RedirectionTableEntry};
use x2apic::lapic::{LocalApic, LocalApicBuilder, TimerMode};
use x86_64::VirtAddr;
use x86_64::{instructions::port::Port, PhysAddr};

use super::interrupts::InterruptIndex;
use crate::drivers::hpet::HPET;
use crate::memory::convert_physical_to_virtual;

const TIMER_FREQUENCY_HZ: u32 = 200;
const TIMER_CALIBRATION_ITERATION: u32 = 100;
const IOAPIC_INTERRUPT_INDEX_OFFSET: u8 = 32;

pub static IOAPIC: OnceCell<Mutex<IoApic>> = OnceCell::uninit();

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum IrqVector {
    Keyboard = 1,
    Mouse = 12,
}

pub fn init() {
    unsafe {
        disable_pic();
        init_ioapic();
    };
    log::info!("APIC initialized successfully!");
}

#[inline]
pub fn end_of_interrupt() {
    unsafe {
        get_lapic().end_of_interrupt();
    }
}

fn get_lapic_addr() -> VirtAddr {
    let acpi = super::acpi::ACPI.try_get().unwrap();
    let physical_address = PhysAddr::new(acpi.apic_info.local_apic_address as u64);
    let virtual_address = convert_physical_to_virtual(physical_address);
    virtual_address
}

/// Gets the local APIC.
pub fn get_lapic() -> LocalApic {
    LocalApicBuilder::new()
        .timer_vector(InterruptIndex::Timer as usize)
        .timer_mode(TimerMode::OneShot)
        .timer_initial(0)
        .error_vector(InterruptIndex::ApicError as usize)
        .spurious_vector(InterruptIndex::ApicSpurious as usize)
        .set_xapic_base(get_lapic_addr().as_u64())
        .build()
        .unwrap_or_else(|err| panic!("Failed to build local APIC: {:#?}", err))
}

/// Returns the local APIC ID of the current CPU.
pub fn get_lapic_id() -> u32 {
    unsafe {
        LocalApicBuilder::new()
            .timer_vector(InterruptIndex::Timer as usize)
            .timer_mode(TimerMode::OneShot)
            .timer_initial(0)
            .error_vector(InterruptIndex::ApicError as usize)
            .spurious_vector(InterruptIndex::ApicSpurious as usize)
            .set_xapic_base(get_lapic_addr().as_u64())
            .build()
            .unwrap_or_else(|err| panic!("Failed to build local APIC: {:#?}", err))
            .id()
    }
}

unsafe fn disable_pic() {
    Port::<u8>::new(0x21).write(0xff);
    Port::<u8>::new(0xa1).write(0xff);
}

unsafe fn init_ioapic() {
    let acpi = super::acpi::ACPI.try_get().unwrap();
    let physical_address = PhysAddr::new(acpi.apic_info.io_apics[0].address as u64);
    let virtual_address = convert_physical_to_virtual(physical_address);

    let mut ioapic = IoApic::new(virtual_address.as_u64());
    ioapic.init(IOAPIC_INTERRUPT_INDEX_OFFSET);
    IOAPIC.init_once(|| Mutex::new(ioapic));

    ioapic_add_entry(IrqVector::Keyboard, InterruptIndex::Keyboard);
    ioapic_add_entry(IrqVector::Mouse, InterruptIndex::Mouse);
}

unsafe fn ioapic_add_entry(irq: IrqVector, vector: InterruptIndex) {
    let lapic = get_lapic();
    let mut ioapic = IOAPIC.try_get().unwrap().lock();
    let mut entry = RedirectionTableEntry::default();
    entry.set_mode(IrqMode::Fixed);
    entry.set_dest(lapic.id() as u8);
    entry.set_vector(vector as u8);
    ioapic.set_table_entry(irq as u8, entry);
    ioapic.enable_irq(irq as u8);
}

pub unsafe fn calibrate_timer(lapic: &mut LocalApic) {
    let mut lapic_total_ticks = 0;
    let hpet_clock_speed = HPET.clock_speed() as u64;
    let hpet_tick_per_ms = 1_000_000_000_000 / hpet_clock_speed;

    for _ in 0..TIMER_CALIBRATION_ITERATION {
        let next_ms = HPET.get_counter() + hpet_tick_per_ms;
        lapic.set_timer_initial(u32::MAX);
        while HPET.get_counter() < next_ms {}
        lapic_total_ticks += u32::MAX - lapic.timer_current();
    }

    let average_clock_per_ms = lapic_total_ticks / TIMER_CALIBRATION_ITERATION;

    lapic.set_timer_mode(TimerMode::Periodic);
    lapic.set_timer_initial(average_clock_per_ms * 1000 / TIMER_FREQUENCY_HZ);
}
