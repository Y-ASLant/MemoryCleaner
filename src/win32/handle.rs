use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Registry::{HKEY, RegCloseKey};

/// Owns a closeable Win32 `HANDLE` and closes it on drop.
#[derive(Debug)]
pub(crate) struct OwnedWin32Handle(OwnedHandle);

impl OwnedWin32Handle {
    /// Takes ownership of a newly returned Win32 handle.
    ///
    /// The caller must transfer a valid, uniquely owned, closeable handle.
    pub(crate) unsafe fn from_raw(handle: HANDLE) -> Self {
        debug_assert!(!handle.is_invalid());
        Self(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    }

    pub(crate) fn raw(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }
}

/// Owns a registry key and closes it with `RegCloseKey` on drop.
#[derive(Debug)]
pub(crate) struct OwnedRegistryKey(HKEY);

impl OwnedRegistryKey {
    /// Takes ownership of a newly opened or created registry key.
    ///
    /// The caller must transfer a valid, uniquely owned registry key.
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
