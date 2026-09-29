//! The host edge every WebAssembly module in this workspace shares.
//!
//! A host copies its input into blocks from `alloc`, calls the module's one
//! reading export, and reads back a frame: a little-endian `u32` length, then
//! that many bytes of JSON. It frees each block and the frame with `dealloc`,
//! passing the length each was allocated with; a frame's is 4 plus its prefix.
//!
//! A `#[no_mangle]` function is exported by the crate that defines it, so
//! [`exports!`] writes `alloc` and `dealloc` into each module, over [`alloc`]
//! and [`dealloc`] here. The reading export stays the module's own.

use serde::Serialize;

/// Reserve `len` bytes for the host to write into.
pub fn alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast()
}

/// Free the `len` bytes at `ptr`.
///
/// # Safety
/// `ptr` and `len` must name one live block: a block from [`alloc`]`(len)`, or
/// a frame from [`frame`] with `len` = 4 + its length prefix.
pub unsafe fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// The `len` bytes the host wrote at `ptr`.
///
/// # Safety
/// `ptr..ptr + len` must be initialized memory, such as a block from [`alloc`],
/// that stays live while the slice does.
pub unsafe fn input<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Serialize `value` into a new frame, writing the JSON in place after the
/// length prefix rather than copying it there.
pub fn frame(value: &impl Serialize) -> *mut u8 {
    let mut out = vec![0u8; 4];
    serde_json::to_writer(&mut out, value).expect("a module's response always serializes");
    let len = u32::try_from(out.len() - 4).expect("a frame fits the wasm32 address space");
    out[..4].copy_from_slice(&len.to_le_bytes());
    // `dealloc` frees exactly `4 + len` bytes, so the block must be that size.
    Box::into_raw(out.into_boxed_slice()).cast()
}

/// Write the `alloc` and `dealloc` exports into the calling crate.
#[macro_export]
macro_rules! exports {
    () => {
        /// Reserve `len` bytes for the host to write into. Free them with
        /// `dealloc(ptr, len)`.
        #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
        pub extern "C" fn alloc(len: usize) -> *mut u8 {
            $crate::alloc(len)
        }

        /// Free the `len` bytes at `ptr`.
        ///
        /// # Safety
        /// `ptr` and `len` must name one live block: a block from `alloc(len)`,
        /// or a frame with `len` = 4 + its length prefix.
        #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
        pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
            unsafe { $crate::dealloc(ptr, len) }
        }
    };
}

/// What a host does, for the modules' native tests.
pub mod host {
    /// Copy `bytes` into a block from [`alloc`](crate::alloc).
    pub fn put(bytes: &[u8]) -> *mut u8 {
        let ptr = crate::alloc(bytes.len());
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        ptr
    }

    /// Read a frame's JSON, then free the frame.
    ///
    /// # Safety
    /// `frame` must be a live frame from [`frame`](crate::frame).
    pub unsafe fn take(frame: *mut u8) -> Vec<u8> {
        unsafe {
            let len = u32::from_le_bytes(*frame.cast::<[u8; 4]>()) as usize;
            let json = std::slice::from_raw_parts(frame.add(4), len).to_vec();
            crate::dealloc(frame, 4 + len);
            json
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_the_length_then_the_json() {
        let value = serde_json::json!({ "text": "a \"quoted\" line\n" });
        let json = unsafe { host::take(frame(&value)) };
        assert_eq!(json, serde_json::to_vec(&value).unwrap());
    }

    #[test]
    fn a_block_holds_what_the_host_wrote() {
        let ptr = host::put(b"bytes");
        assert_eq!(unsafe { input(ptr, 5) }, b"bytes");
        unsafe { dealloc(ptr, 5) };
    }

    #[test]
    fn an_empty_block_round_trips() {
        let ptr = alloc(0);
        assert_eq!(unsafe { input(ptr, 0) }, b"");
        unsafe { dealloc(ptr, 0) };
    }
}
