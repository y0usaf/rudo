use std::error::Error;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};

use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::QueueHandle;

use crate::contracts::CheckInvariant;
use crate::defaults::APP_NAME;

const BYTES_PER_PIXEL: u32 = 4;
const MAX_SHM_DIMENSION: u32 = 16_384;
const MAX_SHM_BYTES: usize = 512 * 1024 * 1024;

fn shm_layout(width: u32, height: u32) -> Result<(u32, usize), Box<dyn Error>> {
    if width == 0 || height == 0 {
        return Err("zero-sized shm buffer".into());
    }
    if width > MAX_SHM_DIMENSION || height > MAX_SHM_DIMENSION {
        return Err("shm buffer dimensions exceed safe limit".into());
    }

    let stride = width
        .checked_mul(BYTES_PER_PIXEL)
        .ok_or("shm stride overflow")?;
    let len_u64 = u64::from(stride)
        .checked_mul(u64::from(height))
        .ok_or("shm buffer length overflow")?;
    let len = usize::try_from(len_u64).map_err(|_| "shm buffer length overflow")?;

    if width > i32::MAX as u32
        || height > i32::MAX as u32
        || stride > i32::MAX as u32
        || len > i32::MAX as usize
    {
        return Err("shm buffer exceeds Wayland protocol limits".into());
    }
    if len > MAX_SHM_BYTES {
        return Err("shm buffer exceeds maximum size".into());
    }

    Ok((stride, len))
}

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
        requires!(width > 0 && height > 0);
        let (stride, len) = shm_layout(width, height)?;
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
        requires!(!self.map.is_null());
        // SAFETY: self.map is a valid pointer from mmap with length self.len.
        // The backing file (_file) is kept alive for the lifetime of ShmBuffer.
        unsafe { std::slice::from_raw_parts(self.map, self.len) }
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        requires!(!self.map.is_null() && !self.busy);
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
    requires!(size > 0);
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

impl CheckInvariant for ShmBuffer {
    fn check_invariant(&self) {
        invariant!(!self.map.is_null() && self.width > 0 && self.height > 0 && self.len > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::shm_layout;

    #[test]
    fn shm_layout_rejects_zero_and_overflowing_sizes() {
        assert!(shm_layout(0, 1).is_err());
        assert!(shm_layout(1, 0).is_err());
        assert!(shm_layout(u32::MAX, 1).is_err());
    }

    #[test]
    fn shm_layout_accepts_reasonable_sizes() {
        let (stride, len) = shm_layout(1920, 1080).unwrap();
        assert_eq!(stride, 1920 * 4);
        assert_eq!(len, 1920 * 1080 * 4);
    }
}
