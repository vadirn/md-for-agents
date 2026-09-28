//! mdstruct as a WebAssembly module: three exported functions a host calls to
//! parse Markdown in-process.
//!
//! The host copies bytes into a block from `alloc`, calls `parse_json`, and reads
//! back a frame: a little-endian `u32` length, then that many bytes of JSON. The
//! JSON is the line `mdstruct -` prints for the same bytes, without its newline.
//! The host frees the block and the frame with `dealloc`, passing the length each
//! was allocated with. `alloc`, `dealloc`, and the frame come from [`wasm_abi`].
//!
//! Only the wasm32 build exports the functions. Natively they stay plain Rust, so
//! the tests below exercise the contract without a WebAssembly runtime.

use mdstruct::{Options, parse_bytes};

wasm_abi::exports!();

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
    let bytes = unsafe { wasm_abi::input(ptr, len) };
    // The options and path the CLI uses for `mdstruct -`.
    match parse_bytes("-", bytes, &Options { wikilinks: true }) {
        Ok(doc) => wasm_abi::frame(&doc),
        Err(e) => wasm_abi::frame(&serde_json::json!({
            "error": format!("input is not valid UTF-8: {e}")
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One host round trip: copy in, parse, read the frame, free both blocks.
    fn call(input: &[u8]) -> Vec<u8> {
        let ptr = wasm_abi::host::put(input);
        unsafe {
            let out = parse_json(ptr, input.len());
            dealloc(ptr, input.len());
            wasm_abi::host::take(out)
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
