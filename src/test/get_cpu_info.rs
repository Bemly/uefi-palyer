use uefi::boot::{locate_handle_buffer, open_protocol_exclusive, SearchType};
use uefi::{println, Identify};
use uefi::prelude::*;
use uefi::proto::pi::mp::MpServices;
use crate::test::get_smbios::get_cpu_info_by_smbios;

pub fn get_cpu_info() {
    // 如果没有注册控制台需要加上
    // uefi::helpers::init().expect("Failed to init UEFI");
    mp_protocol();

}

fn mp_protocol() {


    // 2. 在定位器中寻找 MpServices 协议
    let mp_services_handle =
        locate_handle_buffer(SearchType::ByProtocol(&MpServices::GUID))
            .expect("No MP Services protocol supporting multiple processors found.");

    if mp_services_handle.len() > 1 {
        log::warn!("Multiple MP Services instances were detected, which may involve multiple CPUs or a complex topology!");
    } // 发现多个 MP Services 实例，可能存在多路 CPU 或复杂的拓扑结构！

    let mut mp_services =
        open_protocol_exclusive::<MpServices>(mp_services_handle.first().unwrap().clone())
            .expect("Unable to open MpServices protocol");

    // BSP 喊话
    let my_index = mp_services.who_am_i().expect("Unable to obtain my processor index");
    let my_info = mp_services.get_processor_info(my_index)
        .expect("Unable to obtain my processor information");

    log::info!(
        "I am the BSP {}, located in the physical slot {}, the kernel {}",
        my_index,
        my_info.location.package,
        my_info.location.core
    ); // 我是逻辑核心 {}, 位于物理插槽 {}, 内核 {}



    // 3. 获取处理器计数 (ProcessorCount)
    let counts = mp_services
        .get_number_of_processors()
        .expect("Unable to obtain the number of processors"); // 无法获取处理器数量

    log::info!("Total number of logic processors: {}", counts.total); // 总逻辑处理器数
    log::info!("Number of currently enabled processors: {}", counts.enabled); //当前启用的处理器数

    // 4. 遍历并获取每个处理器的详细信息 (ProcessorInformation)
    for i in 0..counts.total {
        let info = mp_services
            .get_processor_info(i)
            .expect("Unable to obtain processor information");

        println!("Processor Index: {}, uuid:{}, enabled: {}, healthy: {}, bsp: {}",
                 i, info.processor_id, info.is_enabled(), info.is_healthy(), info.is_bsp());


        // 提取物理位置 (CpuPhysicalLocation)
        let loc = info.location;
        println!(
            "Physical location: Slot={}, Kernel={}, Thread={}",
            loc.package,
            loc.core,
            loc.thread
        ); // 物理位置: 插槽={}, 内核={}, 线程={}
    }

    get_cpu_info_by_smbios();
}

