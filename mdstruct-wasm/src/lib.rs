//! mdstruct as a WebAssembly module: three exported functions a host calls to
//! parse Markdown in-process.
//!
//! The host copies bytes into a block from `alloc`, calls `parse_json`, and reads
//! back a frame: a little-endian `u32` length, then that many bytes of JSON. The
//! JSON is the line `mdstruct -` prints for the same bytes, without its newline.
//! The host frees the block and the frame with `dealloc`, passing the length each
//! was allocated with.
//!
//! Only the wasm32 build exports the functions. Natively they stay plain Rust, so
//! the tests below exercise the contract without a WebAssembly runtime.

use mdstruct::{Options, parse_bytes};

/// Reserve `len` bytes for the host to write into. Free them with
/// `dealloc(ptr, len)`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast()
}

/// Free the `len` bytes at `ptr`.
///
/// # Safety
/// `ptr` and `len` must name one live block: a block from `alloc(len)`, or a
/// frame from `parse_json` with `len` = 4 + its length prefix.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// Parse the `len` bytes at `ptr` as `mdstruct -` parses stdin, and return a
/// frame holding the document's JSON.
///
/// Bytes that are not UTF-8 frame `{"error":"..."}` instead, where the CLI
/// reports the error on stderr.
///
/// # Safety
/// `ptr..ptr + len` must be initialized memory, such as a block from `alloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn parse_json(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    frame(&document_json(bytes))
}

fn document_json(bytes: &[u8]) -> Vec<u8> {
    // The options and path the CLI uses for `mdstruct -`.
    let json = match parse_bytes("-", bytes, &Options { wikilinks: true }) {
        Ok(doc) => serde_json::to_vec(&doc),
        Err(e) => serde_json::to_vec(&serde_json::json!({
            "error": format!("input is not valid UTF-8: {e}")
        })),
    };
    json.expect("a document and an error object always serialize")
}

fn frame(json: &[u8]) -> *mut u8 {
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json);
    Box::into_raw(out.into_boxed_slice()).cast()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One host round trip: copy in, parse, read the frame, free both blocks.
    fn call(input: &[u8]) -> Vec<u8> {
        let ptr = alloc(input.len());
        unsafe {
            std::ptr::copy_nonoverlapping(input.as_ptr(), ptr, input.len());
            let out = parse_json(ptr, input.len());
            dealloc(ptr, input.len());
            let len = u32::from_le_bytes(*out.cast::<[u8; 4]>()) as usize;
            let json = std::slice::from_raw_parts(out.add(4), len).to_vec();
            dealloc(out, 4 + len);
            json
        }
    }

    #[test]
    fn a_frame_holds_the_documents_json() {
        let src = b"---\ndescription: d\n---\n# Title [[Page|alias]]\n\nBody.\n";
        let doc = parse_bytes("-", src, &Options { wikilinks: true }).unwrap();
        assert_eq!(call(src), serde_json::to_vec(&doc).unwrap());
    }

    #[test]
    fn empty_input_parses() {
        let doc = parse_bytes("-", b"", &Options { wikilinks: true }).unwrap();
        assert_eq!(call(b""), serde_json::to_vec(&doc).unwrap());
    }

    #[test]
    fn invalid_utf8_frames_an_error_object() {
        let value: serde_json::Value = serde_json::from_slice(&call(b"# \xff\n")).unwrap();
        let message = value["error"].as_str().unwrap();
        assert!(message.starts_with("input is not valid UTF-8"), "{message}");
    }
}
