use anyhow::{bail, Context, Result};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_NO_BUFFERING, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Memory::SetSystemFileCacheSize;
use windows::Win32::System::IO::DeviceIoControl;

use crate::privileges::enable_privilege;
use crate::win32::nt::{
    nt_set_system_information, InfoClass, MemoryCombineInformationEx, SystemFileCacheInformation64,
    SystemMemoryListCommand,
};

type OptimizeFn = fn() -> Result<()>;
type StepPlan = Vec<(&'static str, OptimizeFn)>;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MemoryAreas: u32 {
        const WORKING_SET               = 1 << 0;
        const SYSTEM_FILE_CACHE         = 1 << 1;
        const MODIFIED_PAGE_LIST        = 1 << 2;
        const STANDBY_LIST              = 1 << 3;
        const STANDBY_LIST_LOW_PRIORITY = 1 << 4;
        const COMBINED_PAGE_LIST        = 1 << 5;
        const MODIFIED_FILE_CACHE       = 1 << 6;
        const REGISTRY_CACHE            = 1 << 7;
    }
}

impl MemoryAreas {
    pub const DEFAULT: Self = Self::WORKING_SET
        .union(Self::SYSTEM_FILE_CACHE)
        .union(Self::MODIFIED_PAGE_LIST)
        .union(Self::STANDBY_LIST)
        .union(Self::COMBINED_PAGE_LIST)
        .union(Self::MODIFIED_FILE_CACHE);
}

struct OptimizeStep {
    area: MemoryAreas,
    name: &'static str,
    run: OptimizeFn,
}

const OPTIMIZE_STEPS: &[OptimizeStep] = &[
    OptimizeStep {
        area: MemoryAreas::WORKING_SET,
        name: "工作集",
        run: optimize_working_set,
    },
    OptimizeStep {
        area: MemoryAreas::SYSTEM_FILE_CACHE,
        name: "系统文件缓存",
        run: optimize_system_file_cache,
    },
    OptimizeStep {
        area: MemoryAreas::MODIFIED_PAGE_LIST,
        name: "已修改页面",
        run: optimize_modified_page_list,
    },
    OptimizeStep {
        area: MemoryAreas::STANDBY_LIST,
        name: "备用列表",
        run: optimize_standby_list_normal,
    },
    OptimizeStep {
        area: MemoryAreas::STANDBY_LIST_LOW_PRIORITY,
        name: "备用列表(低)",
        run: optimize_standby_list_low,
    },
    OptimizeStep {
        area: MemoryAreas::COMBINED_PAGE_LIST,
        name: "合并页面",
        run: optimize_combined_page_list,
    },
    OptimizeStep {
        area: MemoryAreas::MODIFIED_FILE_CACHE,
        name: "已修改文件",
        run: optimize_modified_file_cache,
    },
    OptimizeStep {
        area: MemoryAreas::REGISTRY_CACHE,
        name: "注册表缓存",
        run: optimize_registry_cache,
    },
];

pub fn step_plan(areas: MemoryAreas) -> Result<StepPlan> {
    if areas.is_empty() {
        bail!("no memory areas selected");
    }

    Ok(OPTIMIZE_STEPS
        .iter()
        .filter(|step| areas.contains(step.area))
        .map(|step| (step.name, step.run))
        .collect())
}

fn optimize_working_set() -> Result<()> {
    enable_privilege("SeProfileSingleProcessPrivilege")
        .context("Working Set requires SeProfileSingleProcessPrivilege")?;

    let command = SystemMemoryListCommand::EmptyWorkingSets;

    nt_set_system_information(
        InfoClass::MemoryList,
        &command as *const _ as *mut _,
        std::mem::size_of::<SystemMemoryListCommand>() as u32,
    )
    .context("NtSetSystemInformation (Working Set) failed")?;

    Ok(())
}

fn optimize_system_file_cache() -> Result<()> {
    enable_privilege("SeIncreaseQuotaPrivilege")
        .context("System File Cache requires SeIncreaseQuotaPrivilege")?;

    let cache_info = SystemFileCacheInformation64 {
        minimum_working_set: -1i64,
        maximum_working_set: -1i64,
        ..Default::default()
    };

    nt_set_system_information(
        InfoClass::FileCache,
        &cache_info as *const _ as *mut _,
        std::mem::size_of::<SystemFileCacheInformation64>() as u32,
    )
    .context("NtSetSystemInformation (SystemFileCacheInformation) failed")?;

    unsafe {
        let flush_size: usize = usize::MAX;
        SetSystemFileCacheSize(flush_size, flush_size, 0)
            .context("SetSystemFileCacheSize failed")?;
    }

    Ok(())
}

fn optimize_modified_page_list() -> Result<()> {
    enable_privilege("SeProfileSingleProcessPrivilege")
        .context("Modified Page List requires SeProfileSingleProcessPrivilege")?;

    let command = SystemMemoryListCommand::FlushModifiedList;

    nt_set_system_information(
        InfoClass::MemoryList,
        &command as *const _ as *mut _,
        std::mem::size_of::<SystemMemoryListCommand>() as u32,
    )
    .context("NtSetSystemInformation (Modified Page List) failed")?;

    Ok(())
}

fn optimize_standby_list_normal() -> Result<()> {
    optimize_standby_list(false)
}

fn optimize_standby_list_low() -> Result<()> {
    optimize_standby_list(true)
}

fn optimize_standby_list(low_priority: bool) -> Result<()> {
    enable_privilege("SeProfileSingleProcessPrivilege")
        .context("Standby List requires SeProfileSingleProcessPrivilege")?;

    let command = if low_priority {
        SystemMemoryListCommand::PurgeLowPriorityStandbyList
    } else {
        SystemMemoryListCommand::PurgeStandbyList
    };

    nt_set_system_information(
        InfoClass::MemoryList,
        &command as *const _ as *mut _,
        std::mem::size_of::<SystemMemoryListCommand>() as u32,
    )
    .context("NtSetSystemInformation (Standby List) failed")?;

    Ok(())
}

fn optimize_combined_page_list() -> Result<()> {
    enable_privilege("SeProfileSingleProcessPrivilege")
        .context("Combined Page List requires SeProfileSingleProcessPrivilege")?;

    let combine_info = MemoryCombineInformationEx::default();

    nt_set_system_information(
        InfoClass::CombinePhysicalMemory,
        &combine_info as *const _ as *mut _,
        std::mem::size_of::<MemoryCombineInformationEx>() as u32,
    )
    .context("NtSetSystemInformation (Combined Page List) failed")?;

    Ok(())
}

fn optimize_modified_file_cache() -> Result<()> {
    const IOCTL_RESET_WRITE_ORDER: u32 = 0x000900F8;
    const FSCTL_DISCARD_VOLUME_CACHE: u32 = 0x00090054;

    let mut failed: Vec<char> = Vec::new();

    for drive_letter in get_fixed_drives() {
        let path = format!("\\\\.\\{}:", drive_letter);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe {
            CreateFileW(
                windows::core::PCWSTR(wide.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_NO_BUFFERING,
                None,
            )
        };

        let Ok(h) = handle else {
            failed.push(drive_letter);
            continue;
        };
        if h.is_invalid() {
            failed.push(drive_letter);
            continue;
        }

        let mut bytes_returned = 0u32;
        let mut drive_failed = false;

        // IOCTL_RESET_WRITE_ORDER requires a single-byte input buffer
        unsafe {
            if !DeviceIoControl(
                h,
                IOCTL_RESET_WRITE_ORDER,
                Some(&[0u8] as *const _ as *const _),
                1,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            )
            .is_ok()
            {
                drive_failed = true;
            }
            if !DeviceIoControl(
                h,
                FSCTL_DISCARD_VOLUME_CACHE,
                None,
                0,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            )
            .is_ok()
            {
                drive_failed = true;
            }
            let _ = windows::Win32::Storage::FileSystem::FlushFileBuffers(h);
            let _ = CloseHandle(h);
        }

        if drive_failed {
            failed.push(drive_letter);
        }
    }

    if failed.is_empty() {
        Ok(())
    } else {
        bail!("驱动 {:?} 刷新失败", failed)
    }
}

fn optimize_registry_cache() -> Result<()> {
    use windows::Win32::System::Registry::{
        RegFlushKey, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS,
    };

    unsafe {
        let keys = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_CLASSES_ROOT, HKEY_USERS];
        for key in keys {
            let _ = RegFlushKey(key);
        }
    }

    Ok(())
}

fn get_fixed_drives() -> Vec<char> {
    let mut drives = Vec::new();
    for letter in b'A'..=b'Z' {
        let path = format!("{}:\\", letter as char);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let drive_type = unsafe {
            windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(wide.as_ptr()))
        };
        if drive_type == 3u32 {
            drives.push(letter as char);
        }
    }
    drives
}
