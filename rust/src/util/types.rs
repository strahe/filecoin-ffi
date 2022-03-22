use std::{
    ffi::CStr,
    fmt::Display,
    ops::Deref,
    panic,
    path::{Path, PathBuf},
    ptr::{self, NonNull},
    str::Utf8Error,
};

// `CodeAndMessage` is the trait implemented by `code_and_message_impl
use ffi_toolkit::{CodeAndMessage, FCPResponseStatus};

use super::api::init_log;

/// Owned, fixed size array, allocated on the heap.
#[repr(C)]
pub struct fil_Array<T: Sized> {
    ptr: Option<NonNull<*mut T>>,
    len: usize,
}

impl<T: Clone> Clone for fil_Array<T> {
    fn clone(&self) -> Self {
        let ptr = self.ptr.and_then(|ptr| {
            let bytes = unsafe { std::slice::from_raw_parts(ptr.as_ptr(), self.len) }
                .to_owned()
                .into_boxed_slice();

            NonNull::new(Box::into_raw(bytes).cast())
        });

        fil_Array { ptr, len: self.len }
    }
}

impl<T> Default for fil_Array<T> {
    fn default() -> Self {
        Self { ptr: None, len: 0 }
    }
}

impl<T: Sized> fil_Array<T> {
    pub fn is_null(&self) -> bool {
        self.ptr.is_none() || self.len == 0
    }

    /// Converts this array into a boxed slice, transferring ownership of the memory.
    pub fn into_boxed_slice(self) -> Box<[T]> {
        match self.ptr {
            None => Box::new([]),
            Some(ptr) => {
                let res = unsafe {
                    Box::from_raw(std::slice::from_raw_parts_mut(
                        ptr.as_ptr().cast(),
                        self.len,
                    ))
                };
                // no drop for us
                std::mem::forget(self);
                res
            }
        }
    }
}

// This is needed because of https://github.com/rust-lang/rust/issues/59878
impl<T: Sized> IntoIterator for fil_Array<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        Vec::from(self.into_boxed_slice()).into_iter()
    }
}

/// Owned, fixed size and heap allocated array of bytes.
#[allow(non_camel_case_types)]
pub type fil_Bytes = fil_Array<u8>;

impl<T> From<Vec<T>> for fil_Array<T> {
    fn from(buf: Vec<T>) -> Self {
        if buf.is_empty() {
            return Default::default();
        }
        buf.into_boxed_slice().into()
    }
}

impl<T> From<Box<[T]>> for fil_Array<T> {
    fn from(buf: Box<[T]>) -> Self {
        if buf.is_empty() {
            return Default::default();
        }
        let len = buf.len();
        let ptr = NonNull::new(Box::into_raw(buf).cast());

        Self { ptr, len }
    }
}

impl<T> Drop for fil_Array<T> {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr {
            let _ = unsafe { Box::from_raw(ptr.as_ptr()) };
        }
    }
}

impl<T> Deref for fil_Array<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match self.ptr {
            None => unsafe { std::slice::from_raw_parts(ptr::NonNull::dangling().as_ptr(), 0) },
            Some(ptr) => unsafe { std::slice::from_raw_parts(ptr.as_ptr().cast(), self.len) },
        }
    }
}

impl From<&str> for fil_Bytes {
    fn from(s: &str) -> Self {
        if s.is_empty() {
            return Default::default();
        }
        Box::<str>::from(s).into()
    }
}

impl From<String> for fil_Bytes {
    fn from(s: String) -> Self {
        if s.is_empty() {
            return Default::default();
        }
        s.into_boxed_str().into()
    }
}

impl From<PathBuf> for fil_Bytes {
    fn from(s: PathBuf) -> Self {
        s.to_string_lossy().as_ref().into()
    }
}

impl From<&Path> for fil_Bytes {
    fn from(s: &Path) -> Self {
        s.to_string_lossy().as_ref().into()
    }
}

impl From<Box<str>> for fil_Bytes {
    fn from(s: Box<str>) -> Self {
        if s.is_empty() {
            return Default::default();
        }
        let len = s.len();
        let ptr = NonNull::new(Box::into_raw(s).cast());

        Self { ptr, len }
    }
}

impl fil_Bytes {
    pub fn as_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self)
    }

    pub fn as_path(&self) -> Result<PathBuf, Utf8Error> {
        self.as_str().map(PathBuf::from)
    }
}

#[repr(C)]
#[derive(Clone)]
pub struct fil_Result<T: Sized> {
    pub status_code: FCPResponseStatus,
    pub error_msg: fil_Bytes,
    pub value: T,
}

impl<T: Sized> Deref for fil_Result<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: Sized + Default> Default for fil_Result<T> {
    fn default() -> Self {
        fil_Result {
            status_code: FCPResponseStatus::FCPNoError,
            error_msg: Default::default(),
            value: Default::default(),
        }
    }
}

impl<T: Sized> CodeAndMessage for fil_Result<T> {
    fn set_error(&mut self, code_and_message: (FCPResponseStatus, *mut libc::c_char)) {
        let s = unsafe { CStr::from_ptr(code_and_message.1.cast()) }
            .to_string_lossy()
            .to_string();
        self.status_code = code_and_message.0;
        self.error_msg = s.into();
    }
}

impl<T, E> From<Result<T, E>> for fil_Result<T>
where
    T: Sized + Default,
    E: Display,
{
    fn from(r: Result<T, E>) -> Self {
        match r {
            Ok(value) => Self::ok(value),
            Err(e) => Self::err(e.to_string()),
        }
    }
}

impl<T> From<T> for fil_Result<T>
where
    T: Sized,
{
    fn from(value: T) -> Self {
        Self {
            status_code: FCPResponseStatus::FCPNoError,
            error_msg: Default::default(),
            value,
        }
    }
}

impl<T: Sized> fil_Result<T> {
    pub fn ok(value: T) -> Self {
        fil_Result {
            status_code: FCPResponseStatus::FCPNoError,
            error_msg: Default::default(),
            value,
        }
    }

    pub unsafe fn into_boxed_raw(self) -> *mut fil_Result<T> {
        Box::into_raw(Box::new(self))
    }

    pub fn err_with_default(err: impl Into<fil_Bytes>, value: T) -> Self {
        fil_Result {
            status_code: FCPResponseStatus::FCPUnclassifiedError,
            error_msg: err.into(),
            value,
        }
    }
}

impl<T: Sized + Default> fil_Result<T> {
    pub fn err(err: impl Into<fil_Bytes>) -> Self {
        fil_Result {
            status_code: FCPResponseStatus::FCPUnclassifiedError,
            error_msg: err.into(),
            value: Default::default(),
        }
    }
}

#[allow(non_camel_case_types)]
pub type fil_GpuDeviceResponse = fil_Result<fil_Array<fil_Bytes>>;

#[no_mangle]
pub unsafe extern "C" fn fil_destroy_gpu_device_response(ptr: *mut fil_GpuDeviceResponse) {
    let _ = Box::from_raw(ptr);
}

#[allow(non_camel_case_types)]
pub type fil_InitLogFdResponse = fil_Result<()>;

#[no_mangle]
pub unsafe extern "C" fn fil_destroy_init_log_fd_response(ptr: *mut fil_InitLogFdResponse) {
    let _ = Box::from_raw(ptr);
}

/// Catch panics and return an error response
pub fn catch_panic_response<F, T>(name: &str, callback: F) -> *mut fil_Result<T>
where
    T: Sized + Default,
    F: FnOnce() -> anyhow::Result<T>,
{
    // Using AssertUnwindSafe is code smell. Though catching our panics here is really
    // last resort, so it should be OK.
    let result = match panic::catch_unwind(panic::AssertUnwindSafe(|| {
        init_log();
        log::info!("{}: start", name);
        let res = callback();
        log::info!("{}: end", name);
        res
    })) {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(err)) => Err(err.to_string()),
        Err(panic) => {
            let error_msg = match panic.downcast_ref::<&'static str>() {
                Some(message) => message,
                _ => "no unwind information",
            };

            Err(format!("Rust panic: {}", error_msg))
        }
    };

    unsafe { fil_Result::<T>::from(result).into_boxed_raw() }
}
