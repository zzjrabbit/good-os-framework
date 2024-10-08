use core::sync::atomic::Ordering;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use limine::request::SmpRequest;
use limine::response::SmpResponse;
use limine::smp::Cpu;
use spin::{Lazy, RwLock};

use super::apic::calibrate_timer;
use super::gdt::CpuInfo;
use super::interrupts::IDT;
use crate::arch::apic::get_lapic;
use crate::drivers::hpet::HPET_INIT;
use crate::task::scheduler::SCHEDULER_INIT;
use crate::{user, START_SCHEDULE};

#[used]
#[link_section = ".requests"]
static SMP_REQUEST: SmpRequest = SmpRequest::new();

pub static CPUS: Lazy<RwLock<Cpus>> = Lazy::new(|| RwLock::new(Cpus::new()));
pub static BSP_LAPIC_ID: Lazy<u32> = Lazy::new(|| SMP_RESPONSE.bsp_lapic_id());
static SMP_RESPONSE: Lazy<&SmpResponse> = Lazy::new(|| SMP_REQUEST.get_response().unwrap());

unsafe extern "C" fn ap_entry(smp_info: &Cpu) -> ! {

    CPUS.read().get(smp_info.lapic_id).load();
    IDT.load();

    while !HPET_INIT.load(Ordering::SeqCst) {}

    let mut lapic = get_lapic();
    lapic.enable();
    calibrate_timer(&mut lapic);
    lapic.enable_timer();

    while !SCHEDULER_INIT.load(Ordering::SeqCst) {}

    user::init();

    while !START_SCHEDULE.load(Ordering::SeqCst) {}
    x86_64::instructions::interrupts::enable();

    loop {
        x86_64::instructions::hlt();
        //crate::serial_print!(".");
    }
}

pub struct Cpus(BTreeMap<u32, &'static mut CpuInfo>);

impl Cpus {
    pub fn get(&self, lapic_id: u32) -> &CpuInfo {
        self.0.get(&lapic_id).unwrap()
    }

    pub fn get_mut(&mut self, lapic_id: u32) -> &mut CpuInfo {
        self.0.get_mut(&lapic_id).unwrap()
    }

    pub fn iter_id(&self) -> impl Iterator<Item = &u32> {
        self.0.keys()
    }
}

impl Cpus {
    pub fn new() -> Self {
        let mut cpus = BTreeMap::new();
        cpus.insert(*BSP_LAPIC_ID, Box::leak(Box::new(CpuInfo::new())));
        Cpus(cpus)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn init_bsp(&mut self) {
        let bsp_info = self.get_mut(*BSP_LAPIC_ID);
        bsp_info.init();
        bsp_info.load();
    }

    pub fn init_ap(&mut self) {
        for cpu in SMP_RESPONSE.cpus() {
            if cpu.id == *BSP_LAPIC_ID {
                continue;
            }
            let info = Box::leak(Box::new(CpuInfo::new()));
            info.init();
            self.0.insert(cpu.lapic_id, info);
            cpu.goto_address.write(ap_entry);
        }
    }
}
