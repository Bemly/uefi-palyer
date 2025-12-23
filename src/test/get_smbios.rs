use core::slice;
use dmidecode::{EntryPoint, Structure};
use dmidecode::cache::{CacheAssociativity, CacheSize, CacheSize2};
use uefi::println;
use uefi::system::with_config_table;
use uefi::table::cfg::ConfigTableEntry as cfg;

// 2.1.1 SMBIOS Structure Table Entry Point => 0x1F
// source:https://www.dmtf.org/sites/default/files/standards/documents/DSP0130.pdf
const SMBIOS_BUF_LEN: usize = 0x1F;

pub fn get_smbios() {
    println!("Collecting DMI Information...");

    with_config_table(|slice| {
        // 获取smbios的内存地址
        let mut smbios_address = None;
        for i in slice {
            match i.guid {
                cfg::SMBIOS3_GUID => { smbios_address = Some(i.address); break; }
                cfg::SMBIOS_GUID => smbios_address = Some(i.address),
                _ => {}
            }
        }

        if let Some(address) = smbios_address {
            println!("Found smbios address: {:?}", address);
            let buf = unsafe { slice::from_raw_parts(address as *const u8, SMBIOS_BUF_LEN) };
            if let Ok(entry_point) = EntryPoint::search(buf) {
                // 通过终结入口点找到DMI信息表
                let dmi = unsafe { slice::from_raw_parts(
                    entry_point.smbios_address() as *const u8,
                    entry_point.smbios_len() as usize
                )};

                for table in entry_point.structures(&dmi) {
                    if let Ok(t) = table { println!("{:?}", t) }
                }
            }
        } else {
            println!("SMBIOS not found.");
        }
    });
}

pub fn get_cpu_info_by_smbios() {
    with_config_table(|slice| {
        // 获取smbios的内存地址
        let mut smbios_address = None;
        for i in slice {
            match i.guid {
                cfg::SMBIOS3_GUID => { smbios_address = Some(i.address); break; }
                cfg::SMBIOS_GUID => smbios_address = Some(i.address),
                _ => {}
            }
        }

        if let Some(address) = smbios_address {
            println!("Found smbios address: {:?}", address);
            let buf = unsafe { slice::from_raw_parts(address as *const u8, SMBIOS_BUF_LEN) };
            if let Ok(entry_point) = EntryPoint::search(buf) {
                // 通过终结入口点找到DMI信息表
                let dmi = unsafe { slice::from_raw_parts(
                    entry_point.smbios_address() as *const u8,
                    entry_point.smbios_len() as usize
                )};

                // 获取CPU信息
                entry_point.structures(&dmi).flatten()
                    .filter_map(|table| match table {
                        Structure::Processor(cpu) => Some(cpu),
                        _ => None,
                    })
                    .for_each(|cpu| {
                        println!("handle: {}", cpu.handle);
                        println!("socket_designation: {}", cpu.socket_designation);
                        println!("processor_type: {:?}", cpu.processor_type);
                        println!("processor_family: {:?}", cpu.processor_family);
                        println!("processor_manufacturer: {}", cpu.processor_manufacturer);
                        println!("processor_id: {:#018X}", cpu.processor_id); // 以 16 进制格式打印 ID
                        println!("processor_version: {}", cpu.processor_version);
                        println!("voltage: {:?}", cpu.voltage);
                        println!("external_clock: {} MHz", cpu.external_clock);
                        println!("max_speed: {} MHz", cpu.max_speed);
                        println!("current_speed: {} MHz", cpu.current_speed);
                        println!("status: {:?}", cpu.status);
                        println!("processor_upgrade: {:?}", cpu.processor_upgrade);
                        println!("serial_number: {}", cpu.serial_number.unwrap_or("None"));
                        println!("asset_tag: {}", cpu.asset_tag.unwrap_or("None"));
                        println!("part_number: {}", cpu.part_number.unwrap_or("None"));

                        if let Some(count) = cpu.core_count { println!("core_count: {}", count); }
                        if let Some(enabled) = cpu.core_enabled { println!("core_enabled: {}", enabled); }
                        if let Some(threads) = cpu.thread_count { println!("thread_count: {}", threads); }

                        if let Some(charact) = cpu.processor_characteristics {
                            println!("processor_characteristics: {:?}", charact);
                        }

                        if let Some(l1) = cpu.l1_cache_handle { println!("l1_cache_handle: {:#X}", l1); }
                        if let Some(l2) = cpu.l2_cache_handle { println!("l2_cache_handle: {:#X}", l2); }
                        if let Some(l3) = cpu.l3_cache_handle { println!("l3_cache_handle: {:#X}", l3); }

                        // 最丑陋的处理三缓方式
                        entry_point.structures(&dmi).flatten()
                            .filter_map(|table| match table {
                                Structure::Cache(cache) => Some(cache),
                                _ => None,
                            })
                            .for_each(|cache| {
                                let label = if Some(cache.handle) == cpu.l1_cache_handle {
                                    Some("L1")
                                } else if Some(cache.handle) == cpu.l2_cache_handle {
                                    Some("L2")
                                } else if Some(cache.handle) == cpu.l3_cache_handle {
                                    Some("L3")
                                } else {
                                    None
                                };

                                let safe_get_kb = |s1: &CacheSize, s2: &Option<CacheSize2>| -> u64 {
                                    let s1_kb = match s1 {
                                        CacheSize::Granularity1K(sz) => *sz as u64,
                                        CacheSize::Granularity64K(sz) => (*sz as u64) * 64,
                                    };

                                    // 规范：如果旧字段不是 0xFFFF (即 65535)，则它一定是准确的
                                    // 注意：这里的 65535 是指原始 U16 值为 0xFFFF，
                                    // 如果是 Granularity64K(0x7FFF) 或者类似封顶值也算
                                    // 只要旧字段算出来不代表“溢出”就信任
                                    // 2047 是旧版 SMBIOS 规范能表示的最大 MB 数
                                    if s1_kb < 2047 * 1024 {
                                        s1_kb
                                    } else {
                                        // 只有旧字段可能溢出时，才看新字段
                                        match s2 {
                                            Some(CacheSize2::Granularity1K(sz)) => *sz as u64,
                                            Some(CacheSize2::Granularity64K(sz)) => (*sz as u64) * 64,
                                            None => s1_kb, // 死马当活马医用不安全的
                                        }
                                    }
                                };

                                // 如果匹配成功，则按照一行格式打印
                                if let Some(name) = label {

                                    let max_kb = safe_get_kb(&cache.maximum_cache_size, &cache.maximum_cache_size_2);
                                    let inst_kb = safe_get_kb(&cache.installed_size, &cache.installed_size_2);
                                    let core = cpu.core_count.unwrap_or(1) as u64;

                                    println!(
                                        "{} Cache: Installed {:.2}MB({}Kbx{}core) | Max {:.2}MB({}Kbx{}core)| Config: {:?} | Assoc: {:?}",
                                        name,
                                        (inst_kb as f64) / 1024.0,
                                        inst_kb / core, core,
                                        (max_kb as f64) / 1024.0,
                                        max_kb / core, core,
                                        cache.cache_configuration,
                                        cache.associativity.unwrap_or(CacheAssociativity::Unknown)
                                    );
                                }
                            })
                    });


            }
        } else {
            println!("SMBIOS not found.");
        }
    });
}
