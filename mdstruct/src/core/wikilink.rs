//! Decomposes comrak's opaque `WikiLink.url` into `{page, heading, block}`.
//!
//! The fragment conventions (`#heading`, `#^block`) are PKM-flavored, so those
//! fields are `None` for sources that do not use them. Embeds never reach here:
//! comrak emits no WikiLink for `![[…]]`, since the `!` opens an image and
//! swallows the inner `[[`, so a pre-pass flags them instead.

/// The `{page, heading, block}` decomposition of a wikilink target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiTarget {
    pub page: String,
    pub heading: Option<String>,
    pub block: Option<String>,
}

/// Split `url` on its first `#`: everything before is the page; the remainder
/// is a `^block` reference or, failing the caret, a heading fragment.
pub fn decompose(url: &str) -> WikiTarget {
    match url.split_once('#') {
        None => WikiTarget {
            page: url.to_string(),
            heading: None,
            block: None,
        },
        Some((page, frag)) => {
            let (heading, block) = match frag.strip_prefix('^') {
                Some(block) => (None, Some(block.to_string())),
                None if frag.is_empty() => (None, None),
                None => (Some(frag.to_string()), None),
            };
            WikiTarget {
                page: page.to_string(),
                heading,
                block,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_page() {
        assert_eq!(
            decompose("Note"),
            WikiTarget {
                page: "Note".into(),
                heading: None,
                block: None
            }
        );
    }

    #[test]
    fn heading_fragment() {
        assert_eq!(
            decompose("Note#Section"),
            WikiTarget {
                page: "Note".into(),
                heading: Some("Section".into()),
                block: None
            }
        );
    }

    #[test]
    fn block_fragment() {
        assert_eq!(
            decompose("Note#^block-id"),
            WikiTarget {
                page: "Note".into(),
                heading: None,
                block: Some("block-id".into())
            }
        );
    }

    #[test]
    fn same_page_heading() {
        assert_eq!(
            decompose("#Heading"),
            WikiTarget {
                page: "".into(),
                heading: Some("Heading".into()),
                block: None
            }
        );
    }

    #[test]
    fn path_page_with_heading() {
        assert_eq!(
            decompose("projects/alpha/Overview#Direction"),
            WikiTarget {
                page: "projects/alpha/Overview".into(),
                heading: Some("Direction".into()),
                block: None,
            }
        );
    }
}
