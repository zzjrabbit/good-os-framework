pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod interrupts;
pub mod smp;

use acpi::ACPI;
use x86_64::instructions::port::Port;

use crate::drivers::pci::{
    BusDeviceFunction, PciError, PciRoot, SegmentGroupNumber, PORT_PCI_CONFIG_ADDRESS,
    PORT_PCI_CONFIG_DATA,
};

/// TraitPciArch Pci架构相关函数，任何架构都应独立实现trait里的函数
pub trait TraitPciArch {
    /// @brief 读取寄存器值，x86_64架构通过读取两个特定io端口实现
    /// @param bus_device_function 设备的唯一标识符
    /// @param offset 寄存器偏移值
    /// @return 读取到的值
    fn read_config(bus_device_function: &BusDeviceFunction, offset: u8) -> u32;
    /// @brief 写入寄存器值，x86_64架构通过读取两个特定io端口实现
    /// @param bus_device_function 设备的唯一标识符
    /// @param offset 寄存器偏移值
    /// @param data 要写入的值
    fn write_config(bus_device_function: &BusDeviceFunction, offset: u8, data: u32);
    /// @brief PCI域地址到存储器域地址的转换,x86_64架构为一一对应
    /// @param address PCI域地址
    /// @return  Result<usize, PciError> 转换结果或出错原因
    fn address_pci_to_address_memory(address: usize) -> Result<usize, PciError>;
    /// @brief 获取Segement的root地址,x86_64架构为acpi mcfg表中读取
    /// @param segement 组id
    /// @return  Result<PciRoot, PciError> 转换结果或出错原因
    fn ecam_root(segement: SegmentGroupNumber) -> Result<PciRoot, PciError>;
}

pub struct X86_64PciArch;

impl TraitPciArch for X86_64PciArch {
    fn read_config(bus_device_function: &BusDeviceFunction, offset: u8) -> u32 {
        // 构造pci配置空间地址
        let address = ((bus_device_function.bus as u32) << 16)
            | ((bus_device_function.device as u32) << 11)
            | ((bus_device_function.function as u32 & 7) << 8)
            | (offset & 0xfc) as u32
            | (0x80000000);
        let ret = unsafe {
            Port::<u32>::new(PORT_PCI_CONFIG_ADDRESS).write(address);
            let temp = Port::<u32>::new(PORT_PCI_CONFIG_DATA).read();
            temp
        };
        return ret;
    }

    fn write_config(bus_device_function: &BusDeviceFunction, offset: u8, data: u32) {
        let address = ((bus_device_function.bus as u32) << 16)
            | ((bus_device_function.device as u32) << 11)
            | ((bus_device_function.function as u32 & 7) << 8)
            | (offset & 0xfc) as u32
            | (0x80000000);
        unsafe {
            Port::<u32>::new(PORT_PCI_CONFIG_ADDRESS).write(address);
            Port::<u32>::new(PORT_PCI_CONFIG_DATA).write(data);
        }
    }

    fn address_pci_to_address_memory(address: usize) -> Result<usize, PciError> {
        Ok(address)
    }

    fn ecam_root(segement: SegmentGroupNumber) -> Result<PciRoot, PciError> {
        let mcfg_info = ACPI.try_get().unwrap().mcfg_info.clone();

        for segmentgroupconfiguration in mcfg_info {
            if segmentgroupconfiguration.pci_segment_group == segement {
                return Ok(PciRoot {
                    physical_address_base: segmentgroupconfiguration.base_address,
                    mmio_base: None,
                    segement_group_number: segement,
                    bus_begin: segmentgroupconfiguration.bus_number_start,
                    bus_end: segmentgroupconfiguration.bus_number_end,
                });
            }
        }
        return Err(PciError::SegmentNotFound);
    }
}

pub use X86_64PciArch as PciArch;
