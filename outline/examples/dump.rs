//! Print the outline of each file as JSON, for a differential test against
//! the Python spikes the outliners were ported from.
//!
//! ```text
//! cargo run --release -p outline --example dump -- <rs|ts|py|rb> <files...>
//! ```
//!
//! Output: `{path: [[start, end, [[cstart, cend, []], ...]], ...]}`. Ruby's
//! spans print flat, sorted by start ascending and end descending.

use outline::{Lang, Node, outline};
use serde_json::{Map, Value, json};

fn tree(nodes: &[Node]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|n| json!([n.start, n.end, tree(&n.children)]))
            .collect(),
    )
}

/// Pre-order, which is the order `nest` consumed the sorted flat spans in.
fn flat(nodes: &[Node], out: &mut Vec<Value>) {
    for n in nodes {
        out.push(json!([n.start, n.end, []]));
        flat(&n.children, out);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let lang = match args.next().as_deref() {
        Some("rs" | "rust") => Lang::Rust,
        Some("ts" | "typescript") => Lang::TypeScript,
        Some("py" | "python") => Lang::Python,
        Some("rb" | "ruby") => Lang::Ruby,
        _ => {
            eprintln!("usage: dump <rs|ts|py|rb> <files...>");
            std::process::exit(2);
        }
    };
    let mut out = Map::new();
    for path in args {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let text = String::from_utf8_lossy(&bytes);
        let nodes = outline(lang, &text);
        let value = if lang == Lang::Ruby {
            let mut spans = Vec::new();
            flat(&nodes, &mut spans);
            Value::Array(spans)
        } else {
            tree(&nodes)
        };
        out.insert(path, value);
    }
    println!("{}", Value::Object(out));
}
