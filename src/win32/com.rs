use std::marker::PhantomData;
use std::rc::Rc;

use anyhow::{Context, Result};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

/// 在同一线程上配对一次成功的 STA 初始化。
/// 须声明在 COM 对象之前，以便对象先于套间释放。
pub(crate) struct ComApartment {
    // COM 初始化是线程局部的：移动或共享此守卫均不安全。
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComApartment {
    /// 只返回自有数据；COM 对象须留在闭包内。
    pub(crate) fn run<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let _com = Self::initialize()?;
        operation().map_err(|error| {
            // windows::Error 可能持有 IErrorInfo。在释放套间前拆出 HRESULT 与诊断信息，
            // 含提前返回路径。
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
        // S_OK 与 S_FALSE 均占用一次初始化；失败则不占用。
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
