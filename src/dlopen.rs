use std::ffi::c_void;
use std::mem::{self, ManuallyDrop};
use std::ptr;

pub(crate) struct DlLibrary {
    handle: *mut c_void,
}

impl DlLibrary {
    pub(crate) fn open_any(names: &[&[u8]]) -> Option<Self> {
        for name in names {
            let handle =
                unsafe { libc::dlopen(name.as_ptr().cast(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if !handle.is_null() {
                return Some(Self { handle });
            }
        }
        None
    }

    pub(crate) unsafe fn symbol_raw(&self, name: &[u8]) -> Option<*mut c_void> {
        let symbol = unsafe { libc::dlsym(self.handle, name.as_ptr().cast()) };
        if symbol.is_null() {
            return None;
        }
        Some(symbol)
    }

    pub(crate) fn into_raw(mut self) -> *mut c_void {
        let handle = self.handle;
        self.handle = ptr::null_mut();
        mem::forget(self);
        handle
    }
}

impl Drop for DlLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                libc::dlclose(self.handle);
            }
        }
    }
}

pub(crate) struct Symbol<T> {
    name: &'static [u8],
    _marker: std::marker::PhantomData<T>,
}

impl<T> Symbol<T> {
    pub(crate) const fn new(name: &'static [u8]) -> Self {
        Self {
            name,
            _marker: std::marker::PhantomData,
        }
    }

    pub(crate) unsafe fn get(&self, library: &DlLibrary) -> Option<T> {
        let symbol = unsafe { library.symbol_raw(self.name) }?;
        Some(unsafe { symbol_cast(symbol) })
    }
}

union SymbolCast<T> {
    raw: *mut c_void,
    typed: ManuallyDrop<T>,
}

unsafe fn symbol_cast<T>(ptr: *mut c_void) -> T {
    assert_eq!(mem::size_of::<T>(), mem::size_of::<*mut c_void>());
    unsafe { ManuallyDrop::into_inner(SymbolCast { raw: ptr }.typed) }
}
