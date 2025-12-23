use core::slice;
use dmidecode::EntryPoint;
use uefi::println;
use uefi::system::with_config_table;
use uefi::table::cfg::ConfigTableEntry as cfg;


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
            println!("Found SMBIOS3 GUID: {:?}", address);
            // 2.1.1 SMBIOS Structure Table Entry Point => 0x1F
            // source:https://www.dmtf.org/sites/default/files/standards/documents/DSP0130.pdf
            let buf = unsafe { slice::from_raw_parts(address as *const u8, 0x1F) };
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
