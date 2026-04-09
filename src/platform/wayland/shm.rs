use std::error::Error;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};

use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::QueueHandle;

use crate::defaults::APP_NAME;

pub(super) struct ShmBuffer {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    len: usize,
    _file: File,
    map: *mut u8,
    pub pool: wl_shm_pool::WlShmPool,
    pub buffer: wl_buffer::WlBuffer,
    pub busy: bool,
    pub age: u32,
}

impl ShmBuffer {
    pub fn new(
        shm: &wl_shm::WlShm,
        width: u32,
        height: u32,
        qh: &QueueHandle<super::WaylandState>,
        idx: usize,
    ) -> Result<Self, Box<dyn Error>> {
        let stride = width * 4;
        let len = (stride * height) as usize;
        let file = create_shm_file(len)?;
        let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            idx,
        );
        // SAFETY: We pass a valid fd from memfd_create, with PROT_READ|PROT_WRITE
        // and MAP_SHARED. The length matches the ftruncated file size. We check
        // for MAP_FAILED before using the pointer.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_fd().as_raw_fd(),
                0,
            )
        } as *mut u8;
        if map as *mut libc::c_void == libc::MAP_FAILED {
            return Err("mmap failed".into());
        }
        Ok(Self {
            width,
            height,
            stride,
            len,
            _file: file,
            map,
            pool,
            buffer,
            busy: false,
            age: 0,
        })
    }

    pub fn pixels(&self) -> &[u8] {
        // SAFETY: self.map is a valid pointer from mmap with length self.len.
        // The backing file (_file) is kept alive for the lifetime of ShmBuffer.
        unsafe { std::slice::from_raw_parts(self.map, self.len) }
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        // SAFETY: self.map is a valid pointer from mmap with length self.len.
        // The backing file (_file) is kept alive for the lifetime of ShmBuffer.
        // The caller must ensure the Wayland compositor has released the buffer
        // (tracked via the `busy` flag) before writing.
        unsafe { std::slice::from_raw_parts_mut(self.map, self.len) }
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
        // SAFETY: self.map was obtained from mmap with self.len bytes.
        // This is the only munmap call, executed once in Drop.
        unsafe {
            libc::munmap(self.map.cast(), self.len);
        }
    }
}

fn create_shm_file(size: usize) -> Result<File, Box<dyn Error>> {
    let name = CString::new(format!("{APP_NAME}-{}", std::process::id()))?;
    // SAFETY: memfd_create with MFD_CLOEXEC creates an anonymous file.
    // We check fd < 0 for failure.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err("memfd_create failed".into());
    }
    // SAFETY: fd is valid from memfd_create. On ftruncate failure,
    // we close the fd to prevent leaks before returning an error.
    if unsafe { libc::ftruncate(fd, size as libc::off_t) } != 0 {
        unsafe {
            libc::close(fd);
        }
        return Err("ftruncate failed".into());
    }
    // SAFETY: fd is a valid file descriptor from memfd_create,
    // successfully ftruncated. File takes ownership.
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(file)
}
