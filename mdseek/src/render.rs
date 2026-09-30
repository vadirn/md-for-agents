//! Print an outcome for an agent: the verdict, then the files, best first,
//! each with the ranges that matched as hints. The best range's text prints
//! inline, so the agent can often answer without another read.

use std::io::{self, Write};

use serde_json::json;

use crate::rank::{Index, Outcome, Ranked};
use crate::sections::{Kind, Section};

/// Lines of text inlined for the best range.
const INLINE: usize = 60;

/// Ranges hinted under each file.
const HINTS: usize = 3;

/// The hits grouped by file, best file first: a file ranks by its best range.
fn by_file<'a>(sections: &[Section], hits: &'a [Ranked], files: usize) -> Vec<Vec<&'a Ranked>> {
    let mut groups: Vec<Vec<&Ranked>> = Vec::new();
    for hit in hits {
        let path = &sections[hit.section].path;
        match groups
            .iter()
            .position(|g| &sections[g[0].section].path == path)
        {
            Some(k) if groups[k].len() < HINTS => groups[k].push(hit),
            Some(_) => {}
            None if groups.len() < files => groups.push(vec![hit]),
            None => {}
        }
    }
    groups
}

pub fn text(
    out: &mut impl Write,
    index: &Index,
    outcome: &Outcome,
    root: &str,
    files: usize,
) -> io::Result<()> {
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
    for (k, group) in by_file(sections, &outcome.hits, files).iter().enumerate() {
        writeln!(out, "{}. {}", k + 1, sections[group[0].section].path)?;
        for (j, hit) in group.iter().enumerate() {
            let s = &sections[hit.section];
            write!(out, "   {}-{}  {}", s.start, s.end, s.label)?;
            if let Some(parent) = s.context.last() {
                write!(out, "  (in {})", clip(parent, 60))?;
            }
            writeln!(out)?;
            if k == 0 && j == 0 {
                excerpt(out, root, s, INLINE)?;
            }
        }
    }
    writeln!(
        out,
        "The file is the finding and each range a hint. `read <file>` prints its outline."
    )?;
    Ok(())
}

pub fn json(
    out: &mut impl Write,
    index: &Index,
    outcome: &Outcome,
    files: usize,
) -> io::Result<()> {
    let sections = index.sections();
    let files: Vec<_> = by_file(sections, &outcome.hits, files)
        .iter()
        .map(|group| {
            let hits: Vec<_> = group
                .iter()
                .map(|h| {
                    let s = &sections[h.section];
                    json!({"start": s.start, "end": s.end, "label": s.label, "score": h.score})
                })
                .collect();
            json!({"path": sections[group[0].section].path, "hits": hits})
        })
        .collect();
    let value = json!({
        "answered": outcome.answered,
        "reason": outcome.reason,
        "coverage": outcome.coverage,
        "elbow": outcome.elbow,
        "suggestions": outcome.suggestions,
        "files": files,
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
        writeln!(out, "     {:>width$}  {}", no + 1, clip(line, 160))?;
    }
    if s.end > last {
        writeln!(out, "     {:>width$}  … {} more lines", "", s.end - last)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn section(path: &str) -> Section {
        Section {
            path: path.to_string(),
            kind: Kind::Markdown,
            start: 1,
            end: 1,
            label: String::new(),
            context: Vec::new(),
            description: String::new(),
            text: String::new(),
            test: false,
        }
    }

    #[test]
    fn files_rank_by_their_best_hit_and_cap_hints_and_files() {
        let paths = ["a", "b", "a", "c", "a", "a", "b", "d"];
        let sections: Vec<Section> = paths.iter().map(|p| section(p)).collect();
        let hits: Vec<Ranked> = (0..paths.len())
            .map(|k| Ranked {
                section: k,
                score: 10.0 - k as f32,
            })
            .collect();
        let groups: Vec<Vec<usize>> = by_file(&sections, &hits, 3)
            .iter()
            .map(|g| g.iter().map(|h| h.section).collect())
            .collect();
        assert_eq!(groups, vec![vec![0, 2, 4], vec![1, 6], vec![3]]);
    }
}
