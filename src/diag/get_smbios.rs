use core::slice;

use dmidecode::cache::{CacheAssociativity, CacheSize, CacheSize2};
use dmidecode::{EntryPoint, Structure};

use crate::diag::common::find_smbios_address;
use crate::emit;
use crate::error::Result;
use crate::graphics::Screen;

// 2.1.1 SMBIOS Structure Table Entry Point => 0x1F
// source:https://www.dmtf.org/sites/default/files/standards/documents/DSP0130.pdf
const SMBIOS_BUF_LEN: usize = 0x1F;

pub fn run(screen: &mut Screen) -> Result {
    get_smbios(screen);
    Ok(())
}

pub fn get_smbios(screen: &mut Screen) {
    emit!(screen, "Collecting DMI Information...");

    let Some(address) = find_smbios_address() else {
        emit!(screen, "SMBIOS not found.");
        return;
    };

    emit!(screen, "Found smbios address: {:?}", address);
    let buf = unsafe { slice::from_raw_parts(address as *const u8, SMBIOS_BUF_LEN) };
    if let Ok(entry_point) = EntryPoint::search(buf) {
        // 通过终结入口点找到DMI信息表
        let dmi = unsafe {
            slice::from_raw_parts(
                entry_point.smbios_address() as *const u8,
                entry_point.smbios_len() as usize,
            )
        };

        for table in entry_point.structures(&dmi).flatten() {
            emit!(screen, "{:?}", table);
        }
    }
}

pub fn get_cpu_info_by_smbios(screen: &mut Screen) {
    let Some(address) = find_smbios_address() else {
        emit!(screen, "SMBIOS not found.");
        return;
    };

    emit!(screen, "Found smbios address: {:?}", address);
    let buf = unsafe { slice::from_raw_parts(address as *const u8, SMBIOS_BUF_LEN) };
    let Ok(entry_point) = EntryPoint::search(buf) else {
        return;
    };

    // 通过终结入口点找到DMI信息表
    let dmi = unsafe {
        slice::from_raw_parts(
            entry_point.smbios_address() as *const u8,
            entry_point.smbios_len() as usize,
        )
    };

    // 获取CPU信息（用 for 循环而非闭包：emit! 需要 &mut screen，
    // 嵌套闭包会造成双重可变借用）
    for table in entry_point.structures(&dmi).flatten() {
        let Structure::Processor(cpu) = table else {
            continue;
        };

        emit!(screen, "handle: {}", cpu.handle);
        emit!(screen, "socket_designation: {}", cpu.socket_designation);
        emit!(screen, "processor_type: {:?}", cpu.processor_type);
        emit!(screen, "processor_family: {:?}", cpu.processor_family);
        emit!(
            screen,
            "processor_manufacturer: {}",
            cpu.processor_manufacturer
        );
        emit!(screen, "processor_id: {:#018X}", cpu.processor_id); // 以 16 进制格式打印 ID
        emit!(screen, "processor_version: {}", cpu.processor_version);
        emit!(screen, "voltage: {:?}", cpu.voltage);
        emit!(screen, "external_clock: {} MHz", cpu.external_clock);
        emit!(screen, "max_speed: {} MHz", cpu.max_speed);
        emit!(screen, "current_speed: {} MHz", cpu.current_speed);
        emit!(screen, "status: {:?}", cpu.status);
        emit!(screen, "processor_upgrade: {:?}", cpu.processor_upgrade);
        emit!(
            screen,
            "serial_number: {}",
            cpu.serial_number.unwrap_or("None")
        );
        emit!(screen, "asset_tag: {}", cpu.asset_tag.unwrap_or("None"));
        emit!(screen, "part_number: {}", cpu.part_number.unwrap_or("None"));

        if let Some(count) = cpu.core_count {
            emit!(screen, "core_count: {}", count);
        }
        if let Some(enabled) = cpu.core_enabled {
            emit!(screen, "core_enabled: {}", enabled);
        }
        if let Some(threads) = cpu.thread_count {
            emit!(screen, "thread_count: {}", threads);
        }

        if let Some(charact) = cpu.processor_characteristics {
            emit!(screen, "processor_characteristics: {:?}", charact);
        }

        if let Some(l1) = cpu.l1_cache_handle {
            emit!(screen, "l1_cache_handle: {:#X}", l1);
        }
        if let Some(l2) = cpu.l2_cache_handle {
            emit!(screen, "l2_cache_handle: {:#X}", l2);
        }
        if let Some(l3) = cpu.l3_cache_handle {
            emit!(screen, "l3_cache_handle: {:#X}", l3);
        }

        // 最丑陋的处理三缓方式
        for cache_table in entry_point.structures(&dmi).flatten() {
            let Structure::Cache(cache) = cache_table else {
                continue;
            };

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

                emit!(
                    screen,
                    "{} Cache: Installed {:.2}MB({}Kbx{}core) | Max {:.2}MB({}Kbx{}core)| Config: {:?} | Assoc: {:?}",
                    name,
                    (inst_kb as f64) / 1024.0,
                    inst_kb / core,
                    core,
                    (max_kb as f64) / 1024.0,
                    max_kb / core,
                    core,
                    cache.cache_configuration,
                    cache.associativity.unwrap_or(CacheAssociativity::Unknown)
                );
            }
        }
    }
}
