//! mdread as a WebAssembly module: three exported functions a host calls to
//! fold and unfold Markdown in-process.
//!
//! The host copies the content into one block from `alloc` and the options into
//! another, calls `read_json`, and reads back a frame: a little-endian `u32`
//! length, then that many bytes of JSON. The content is the bytes `mdread -`
//! reads from stdin, and the options are the arguments that follow it, as JSON:
//! `{"address"?: ..., "depth"?: ..., "full"?: ..., "threshold"?: ...}`. The
//! response is `{"text": ...}`, holding what `mdread - [address]` prints for
//! them. The host frees the blocks and the frame with `dealloc`, passing the
//! length each was allocated with.
//!
//! Only the wasm32 build exports the functions. Natively they stay plain Rust, so
//! the tests below exercise the contract without a WebAssembly runtime.

use mdread::{Dialect, read_content, render};
use serde::Deserialize;

/// The `mdread` binary's inline cutoff when `--threshold` is absent.
const DEFAULT_THRESHOLD: usize = 2000;

/// The arguments after `mdread -`. An omitted option takes the CLI's default,
/// and an unknown one is an error rather than an option silently ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
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

/// Read the content at `content_ptr` as `mdread -` reads its stdin, with the
/// JSON options at `options_ptr` as its arguments, and return a frame holding
/// `{"text": ...}`.
///
/// Where the CLI exits non-zero, the frame holds `{"error": ...}` instead, with
/// the message the CLI prints on stderr. Options that are not JSON of the shape
/// above frame an error too.
///
/// # Safety
/// Both ranges must be initialized memory, such as blocks from `alloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn read_json(
    content_ptr: *const u8,
    content_len: usize,
    options_ptr: *const u8,
    options_len: usize,
) -> *mut u8 {
    let content = unsafe { std::slice::from_raw_parts(content_ptr, content_len) };
    let options = unsafe { std::slice::from_raw_parts(options_ptr, options_len) };
    frame(&response_json(content, options))
}

fn response_json(content: &[u8], options: &[u8]) -> Vec<u8> {
    let response = match read(content, options) {
        Ok(text) => serde_json::json!({ "text": text }),
        Err(error) => serde_json::json!({ "error": error }),
    };
    serde_json::to_vec(&response).expect("an object of one string always serializes")
}

fn read(content: &[u8], options: &[u8]) -> Result<String, String> {
    // In the CLI's order: arguments first, then stdin.
    let options: Options =
        serde_json::from_slice(options).map_err(|e| format!("invalid options: {e}"))?;
    // The read `mdread -` does on stdin, so bytes that are not UTF-8 fail with
    // the CLI's own message.
    let content = std::io::read_to_string(content).map_err(|e| e.to_string())?;
    // The path and dialect the CLI uses for `mdread -` without flags.
    let reading = read_content(
        "-",
        &content,
        options.address.as_deref(),
        options.depth,
        options.full,
        options.threshold.unwrap_or(DEFAULT_THRESHOLD),
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

    /// Copy `bytes` into a block from `alloc`, as the host does.
    fn put(bytes: &[u8]) -> *mut u8 {
        let ptr = alloc(bytes.len());
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        ptr
    }

    /// One host round trip: copy both in, read, read the frame, free all three.
    fn call(content: &[u8], options: &[u8]) -> Value {
        let (content_ptr, options_ptr) = (put(content), put(options));
        unsafe {
            let out = read_json(content_ptr, content.len(), options_ptr, options.len());
            dealloc(content_ptr, content.len());
            dealloc(options_ptr, options.len());
            let len = u32::from_le_bytes(*out.cast::<[u8; 4]>()) as usize;
            let json = std::slice::from_raw_parts(out.add(4), len).to_vec();
            dealloc(out, 4 + len);
            serde_json::from_slice(&json).unwrap()
        }
    }

    fn text(content: &str, options: Value) -> String {
        let response = call(content.as_bytes(), &serde_json::to_vec(&options).unwrap());
        response["text"]
            .as_str()
            .unwrap_or_else(|| panic!("expected text, got {response}"))
            .to_string()
    }

    fn error(content: &[u8], options: &[u8]) -> String {
        let response = call(content, options);
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
            text("# Title\n\nBody.\n", json!({})),
            "-\nlinks: 0\n\n  1      Title          L1   3 lines · ~3 tok\n\nnext: <addr> a section · fm frontmatter (fm.<path> one value) · links outgoing links\n"
        );
        assert_eq!(
            text(DOC, json!({})),
            printed(DOC, None, None, false, DEFAULT_THRESHOLD)
        );
    }

    #[test]
    fn an_address_unfolds_one_section() {
        for address in ["1", "1.2", "two", "0", "fm", "fm.title", "links"] {
            assert_eq!(
                text(DOC, json!({ "address": address })),
                printed(DOC, Some(address), None, false, DEFAULT_THRESHOLD),
                "address {address}"
            );
        }
    }

    #[test]
    fn options_reach_the_reader_as_the_flags_do() {
        let request = |extra: Value| {
            let mut options = json!({ "address": "1" });
            options
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            text(DOC, options)
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
        let defaults = text(DOC, json!({ "address": "1" }));
        assert_eq!(
            text(
                DOC,
                json!({ "address": "1", "depth": null, "full": false, "threshold": 2000 })
            ),
            defaults
        );
        assert_eq!(text(DOC, json!({ "address": null })), text(DOC, json!({})));
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
            let options = json!({ "address": address });
            assert_eq!(
                error(DOC.as_bytes(), &serde_json::to_vec(&options).unwrap()),
                expected
            );
        }
        assert_eq!(
            error(b"# One\n", br#"{"address": "fm"}"#),
            "No frontmatter block in this file (address 'fm')"
        );
    }

    #[test]
    fn content_that_is_not_utf8_frames_the_cli_message() {
        // `printf '# A\n\xff\n' | mdread -`, which exits 1 with this line.
        assert_eq!(
            error(b"# A\n\xff\n", b"{}"),
            "stream did not contain valid UTF-8"
        );
    }

    #[test]
    fn malformed_options_frame_an_error() {
        for options in [
            &b"# not json\n"[..],
            b"",
            br#"{"content": "x"}"#,
            br#"{"depth": -1}"#,
            br#"{"depth": 1.5}"#,
            br#"{"strict_headings": true}"#,
            b"{\"address\": \"\xff\"}",
        ] {
            let message = error(DOC.as_bytes(), options);
            assert!(message.starts_with("invalid options: "), "{message}");
        }
    }

    #[test]
    fn empty_content_reads() {
        assert_eq!(
            text("", json!({})),
            printed("", None, None, false, DEFAULT_THRESHOLD)
        );
    }

    #[test]
    fn a_leading_bom_stays_in_the_content() {
        // `mdread -` keeps a BOM its stdin starts with; the module does too.
        let bommed = format!("\u{feff}{DOC}");
        assert_eq!(
            text(&bommed, json!({ "address": "0" })),
            printed(&bommed, Some("0"), None, false, DEFAULT_THRESHOLD)
        );
    }
}
