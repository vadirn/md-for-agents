//! mdread as a WebAssembly module: three exported functions a host calls to
//! fold and unfold Markdown in-process.
//!
//! The host copies a JSON request into a block from `alloc`, calls `read_json`,
//! and reads back a frame: a little-endian `u32` length, then that many bytes of
//! JSON. The request carries the content `mdread -` reads from stdin and the
//! arguments that follow it:
//! `{"content": ..., "address"?: ..., "depth"?: ..., "full"?: ..., "threshold"?: ...}`.
//! The response is `{"text": ...}`, holding what `mdread - [address]` prints for
//! them. The host frees the block and the frame with `dealloc`, passing the
//! length each was allocated with.
//!
//! Only the wasm32 build exports the functions. Natively they stay plain Rust, so
//! the tests below exercise the contract without a WebAssembly runtime.

use mdread::{Dialect, read_content, render};
use serde::Deserialize;

/// The `mdread` binary's inline cutoff when `--threshold` is absent.
const DEFAULT_THRESHOLD: usize = 2000;

/// One reading. An omitted option takes the CLI's default, and an unknown one
/// is an error rather than an option silently ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    content: String,
    address: Option<String>,
    depth: Option<usize>,
    #[serde(default)]
    full: bool,
    threshold: Option<usize>,
}

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
/// frame from `read_json` with `len` = 4 + its length prefix.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// Read the JSON request in the `len` bytes at `ptr` as `mdread -` reads its
/// stdin and arguments, and return a frame holding `{"text": ...}`.
///
/// Where the CLI exits non-zero, the frame holds `{"error": ...}` instead, with
/// the message the CLI prints on stderr. A request that is not JSON of the shape
/// above frames an error too.
///
/// # Safety
/// `ptr..ptr + len` must be initialized memory, such as a block from `alloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn read_json(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    frame(&response_json(bytes))
}

fn response_json(bytes: &[u8]) -> Vec<u8> {
    let response = match read(bytes) {
        Ok(text) => serde_json::json!({ "text": text }),
        Err(error) => serde_json::json!({ "error": error }),
    };
    serde_json::to_vec(&response).expect("an object of one string always serializes")
}

fn read(bytes: &[u8]) -> Result<String, String> {
    let request: Request =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid request: {e}"))?;
    // The path and dialect the CLI uses for `mdread -` without flags.
    let reading = read_content(
        "-",
        &request.content,
        request.address.as_deref(),
        request.depth,
        request.full,
        request.threshold.unwrap_or(DEFAULT_THRESHOLD),
        Dialect::default(),
    )
    .map_err(|e| e.to_string())?;
    let mut text = Vec::new();
    render::write_text(&mut text, &reading).expect("writing into a Vec cannot fail");
    Ok(String::from_utf8(text).expect("the text is written from strings"))
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
    use serde_json::{Value, json};

    /// One host round trip: copy in, read, read the frame, free both blocks.
    fn call(input: &[u8]) -> Value {
        let ptr = alloc(input.len());
        unsafe {
            std::ptr::copy_nonoverlapping(input.as_ptr(), ptr, input.len());
            let out = read_json(ptr, input.len());
            dealloc(ptr, input.len());
            let len = u32::from_le_bytes(*out.cast::<[u8; 4]>()) as usize;
            let json = std::slice::from_raw_parts(out.add(4), len).to_vec();
            dealloc(out, 4 + len);
            serde_json::from_slice(&json).unwrap()
        }
    }

    fn text(request: Value) -> String {
        let response = call(&serde_json::to_vec(&request).unwrap());
        response["text"]
            .as_str()
            .unwrap_or_else(|| panic!("expected text, got {response}"))
            .to_string()
    }

    fn error(request: &[u8]) -> String {
        let response = call(request);
        response["error"]
            .as_str()
            .unwrap_or_else(|| panic!("expected an error, got {response}"))
            .to_string()
    }

    /// What the CLI prints for these arguments, rendered through the library.
    fn printed(
        content: &str,
        address: Option<&str>,
        depth: Option<usize>,
        full: bool,
        threshold: usize,
    ) -> String {
        let reading = read_content(
            "-",
            content,
            address,
            depth,
            full,
            threshold,
            Dialect::default(),
        )
        .unwrap();
        let mut out = Vec::new();
        render::write_text(&mut out, &reading).unwrap();
        String::from_utf8(out).unwrap()
    }

    const DOC: &str = "---\ntitle: T\n---\n\nLede.\n\n# One\n\nBody.\n\n## Small\n\ntiny.\n\n## Large\n\nLLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL.\n\n### Grand\n\ngrand.\n\n# Two\n\nSee [[Page]].\n";

    #[test]
    fn a_frame_holds_the_overview_the_cli_prints_for_stdin() {
        // `printf '# Title\n\nBody.\n' | mdread -`, byte for byte.
        assert_eq!(
            text(json!({ "content": "# Title\n\nBody.\n" })),
            "-\nlinks: 0\n\n  1      Title          L1   3 lines · ~3 tok\n\nnext: <addr> a section · fm frontmatter (fm.<path> one value) · links outgoing links\n"
        );
        assert_eq!(
            text(json!({ "content": DOC })),
            printed(DOC, None, None, false, DEFAULT_THRESHOLD)
        );
    }

    #[test]
    fn an_address_unfolds_one_section() {
        for address in ["1", "1.2", "two", "0", "fm", "fm.title", "links"] {
            assert_eq!(
                text(json!({ "content": DOC, "address": address })),
                printed(DOC, Some(address), None, false, DEFAULT_THRESHOLD),
                "address {address}"
            );
        }
    }

    #[test]
    fn options_reach_the_reader_as_the_flags_do() {
        let request = |extra: Value| {
            let mut r = json!({ "content": DOC, "address": "1" });
            r.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            text(r)
        };
        assert_eq!(
            request(json!({ "depth": 1 })),
            printed(DOC, Some("1"), Some(1), false, DEFAULT_THRESHOLD)
        );
        assert_eq!(
            request(json!({ "threshold": 5 })),
            printed(DOC, Some("1"), None, false, 5)
        );
        assert_eq!(
            request(json!({ "threshold": 5, "full": true })),
            printed(DOC, Some("1"), None, true, 5)
        );
        // The fixture folds `Large` under a threshold of 5, so the three
        // readings above differ and each option is seen to take effect.
        assert_ne!(request(json!({ "threshold": 5 })), request(json!({})));
        assert_ne!(
            request(json!({ "threshold": 5, "full": true })),
            request(json!({ "threshold": 5 }))
        );
    }

    #[test]
    fn omitted_and_null_options_take_the_cli_defaults() {
        let defaults = text(json!({ "content": DOC, "address": "1" }));
        assert_eq!(
            text(
                json!({ "content": DOC, "address": "1", "depth": null, "full": false, "threshold": 2000 })
            ),
            defaults
        );
        assert_eq!(
            text(json!({ "content": DOC, "address": null })),
            text(json!({ "content": DOC }))
        );
    }

    #[test]
    fn a_failed_reading_frames_the_message_the_cli_prints() {
        for address in ["99", "nope", "fm.missing"] {
            let expected = read_content(
                "-",
                DOC,
                Some(address),
                None,
                false,
                DEFAULT_THRESHOLD,
                Dialect::default(),
            )
            .unwrap_err()
            .to_string();
            let request = json!({ "content": DOC, "address": address });
            assert_eq!(error(&serde_json::to_vec(&request).unwrap()), expected);
        }
        assert_eq!(
            error(br##"{"content": "# One\n", "address": "fm"}"##),
            "No frontmatter block in this file (address 'fm')"
        );
    }

    #[test]
    fn a_malformed_request_frames_an_error() {
        for request in [
            &b"# not json\n"[..],
            br#"{"address": "1"}"#,
            br#"{"content": "x", "depth": -1}"#,
            br#"{"content": "x", "depth": 1.5}"#,
            br#"{"content": "x", "strict_headings": true}"#,
            b"{\"content\": \"\xff\"}",
        ] {
            let message = error(request);
            assert!(message.starts_with("invalid request: "), "{message}");
        }
    }

    #[test]
    fn empty_content_reads() {
        assert_eq!(
            text(json!({ "content": "" })),
            printed("", None, None, false, DEFAULT_THRESHOLD)
        );
    }

    #[test]
    fn a_leading_bom_stays_in_the_content() {
        // `mdread -` keeps a BOM its stdin starts with; the request does too.
        let bommed = format!("\u{feff}{DOC}");
        assert_eq!(
            text(json!({ "content": bommed })),
            printed(&bommed, None, None, false, DEFAULT_THRESHOLD)
        );
    }
}
