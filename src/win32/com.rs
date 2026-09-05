use std::marker::PhantomData;
use std::rc::Rc;

use anyhow::{Context, Result};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

/// Balances one successful STA initialization on the same thread.
/// Declare before COM objects so they are released before the apartment.
pub(crate) struct ComApartment {
    // COM initialization is thread-local: neither moving nor sharing this guard is safe.
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComApartment {
    /// Return owned data only; COM objects must remain inside the operation.
    pub(crate) fn run<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let _com = Self::initialize()?;
        operation().map_err(|error| {
            // windows::Error can own IErrorInfo. Detach its HRESULT and diagnostics
            // before releasing the apartment, including on early-return paths.
            if let Some(native) = error.downcast_ref::<windows::core::Error>() {
                anyhow::Error::new(windows::core::Error::from_hresult(native.code()))
                    .context(format!("{error:#}"))
            } else {
                error
            }
        })
    }

    pub(crate) fn initialize() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .context("CoInitializeEx(STA) failed")?;
        }
        // Both S_OK and S_FALSE own an initialization; failures own nothing.
        Ok(Self {
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
