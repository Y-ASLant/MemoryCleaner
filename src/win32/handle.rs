use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Registry::{HKEY, RegCloseKey};

/// 持有可关闭的 Win32 `HANDLE`，在 drop 时关闭。
#[derive(Debug)]
pub(crate) struct OwnedWin32Handle(OwnedHandle);

impl OwnedWin32Handle {
    /// 接管新返回的 Win32 句柄。
    ///
    /// 调用方须移交有效、唯一持有、可关闭的句柄。
    pub(crate) unsafe fn from_raw(handle: HANDLE) -> Self {
        debug_assert!(!handle.is_invalid());
        Self(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    }

    pub(crate) fn raw(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }
}

/// 持有注册表键，在 drop 时用 `RegCloseKey` 关闭。
#[derive(Debug)]
pub(crate) struct OwnedRegistryKey(HKEY);

impl OwnedRegistryKey {
    /// 接管新打开或新建的注册表键。
    ///
    /// 调用方须移交有效、唯一持有的注册表键。
    pub(crate) unsafe fn from_raw(key: HKEY) -> Self {
        debug_assert!(!key.is_invalid());
        Self(key)
    }

    pub(crate) fn raw(&self) -> HKEY {
        self.0
    }
}

impl Drop for OwnedRegistryKey {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}
