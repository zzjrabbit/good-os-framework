mod cmd;
mod memory;
mod nvme;
mod queues;

use crate::drivers::pci::{get_pci_device_structure_mut, PCI_DEVICE_LINKEDLIST};
use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use memory::Dma;
pub use nvme::{NvmeDevice, NvmeQueuePair};
pub use queues::QUEUE_LENGTH;
use spin::Mutex;

static NVME_CONS: Mutex<Vec<NvmeDevice>> = Mutex::new(Vec::new());
static NVME_SIZES: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());

pub fn init() {
    let mut list = PCI_DEVICE_LINKEDLIST.write();
    let pci_devices = get_pci_device_structure_mut(&mut list, 0x01, 0x08);
    let mut nvme_cons = NVME_CONS.lock();
    let mut nvme_sizes = NVME_SIZES.lock();

    let mut idx = 0;
    for pci_device in pci_devices {
        if let None = pci_device.bar_init() {
            continue;
        }

        if let Ok(bar) = pci_device
            .as_standard_device()
            .unwrap()
            .standard_device_bar
            .get_bar(0)
        {
            if let Some((_, len)) = bar.memory_address_size() {
                let header = bar.virtual_address().unwrap() as usize;

                pci_device.as_mut().enable_master();

                log::info!("NVMe OK");
                let mut nvme_device =
                    NvmeDevice::init(header, len as usize).expect("Cannot init NVMe device");

                nvme_device
                    .identify_controller()
                    .expect("Cannot identify controller");
                let ns = nvme_device.identify_namespace_list(0);

                let mut nvmcap = 0;
                for n in ns {
                    let cap = nvme_device.identify_namespace(n).1 as usize;
                    nvmcap += cap;
                }

                nvme_cons.push(nvme_device);
                log::info!("NVM capacity = {}", nvmcap);
                nvme_sizes.insert(idx, nvmcap);
            }
        }

        idx += 1;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NvmeNamespace {
    pub id: u32,
    pub blocks: u64,
    pub block_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NvmeStats {
    pub completions: u64,
    pub submissions: u64,
}

/// Reads a block from the NVMe driver at block block_id
pub fn read_block(hd: usize, block_id: u64, buf: &mut [u8]) {
    let dma: Dma<u8> = Dma::allocate(buf.len()).expect("Cannot allocate frame");
    let mut cons = NVME_CONS.lock();
    let nvme = cons.get_mut(hd).expect("Cannot get hd");
    nvme.read(&dma, block_id).expect("Cannot read");
    unsafe { buf.as_mut_ptr().copy_from(dma.virt, 512) };
}

/// Writes a block to the NVMe driver at block block_id
pub fn write_block(hd: usize, block_id: u64, buf: &[u8]) {
    let dma: Dma<u8> = Dma::allocate(buf.len()).expect("Cannot allocate frame");
    unsafe { dma.virt.copy_from(buf.as_ptr(), 512) };
    let mut cons = NVME_CONS.lock();
    let nvme = cons.get_mut(hd).expect("Cannot get hd");
    nvme.write(&dma, block_id).expect("Cannot write");
}

/// Gets the number of NVMe drives
pub fn get_hd_num() -> usize {
    let cons = NVME_CONS.lock();
    cons.len()
}

/// Get the driver size of the NVMe driver.
pub fn get_hd_size(hd: usize) -> Option<usize> {
    let cons = NVME_SIZES.lock();
    cons.get(&hd).copied()
}
