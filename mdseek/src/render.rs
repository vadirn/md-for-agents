//! Print an outcome for an agent: the verdict, then the candidates, with the
//! best ones' text inline so the agent can answer without another read.

use std::io::{self, Write};

use serde_json::json;

use crate::rank::{Index, Outcome};
use crate::sections::{Kind, Section};

/// Lines of text inlined for each of the first candidates, best first.
const INLINE: &[usize] = &[60, 20];

pub fn text(out: &mut impl Write, index: &Index, outcome: &Outcome, root: &str) -> io::Result<()> {
    let sections = index.sections();
    if outcome.answered {
        writeln!(out, "answered")?;
    } else {
        write!(out, "no-answer ({})", outcome.reason.unwrap_or("no match"))?;
        if !outcome.suggestions.is_empty() {
            write!(out, "; try adding: {}", outcome.suggestions.join(", "))?;
        }
        writeln!(out)?;
    }
    for (k, hit) in outcome.hits.iter().enumerate() {
        let s = &sections[hit.section];
        write!(
            out,
            "{}. {}:{}-{}  {}",
            k + 1,
            s.path,
            s.start,
            s.end,
            s.label
        )?;
        if let Some(parent) = s.context.last() {
            write!(out, "  (in {})", clip(parent, 60))?;
        }
        writeln!(out)?;
        if let Some(&limit) = INLINE.get(k) {
            excerpt(out, root, s, limit)?;
        }
    }
    writeln!(
        out,
        "Each range is the whole definition or section, so it can be cited as printed."
    )?;
    Ok(())
}

pub fn json(out: &mut impl Write, index: &Index, outcome: &Outcome) -> io::Result<()> {
    let sections = index.sections();
    let hits: Vec<_> = outcome
        .hits
        .iter()
        .map(|h| {
            let s = &sections[h.section];
            json!({"path": s.path, "start": s.start, "end": s.end, "label": s.label, "score": h.score})
        })
        .collect();
    let value = json!({
        "answered": outcome.answered,
        "reason": outcome.reason,
        "coverage": outcome.coverage,
        "elbow": outcome.elbow,
        "suggestions": outcome.suggestions,
        "hits": hits,
    });
    writeln!(out, "{}", value)
}

fn excerpt(out: &mut impl Write, root: &str, s: &Section, limit: usize) -> io::Result<()> {
    let path = std::path::Path::new(root).join(&s.path);
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let width = s.end.to_string().len();
    let last = s.end.min(s.start + limit - 1);
    for (no, line) in content
        .lines()
        .enumerate()
        .skip(s.start - 1)
        .take(last + 1 - s.start)
    {
        let line = if matches!(s.kind, Kind::Markdown) {
            line.trim_end()
        } else {
            line
        };
        writeln!(out, "   {:>width$}  {}", no + 1, clip(line, 160))?;
    }
    if s.end > last {
        writeln!(out, "   {:>width$}  … {} more lines", "", s.end - last)?;
    }
    Ok(())
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max).collect();
    s.push('…');
    s
}
