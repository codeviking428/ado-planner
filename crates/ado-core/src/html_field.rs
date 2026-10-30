use html5ever::driver::ParseOpts;
use html5ever::parse_document;
use html5ever::serialize::{SerializeOpts, serialize};
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{RcDom, SerializableHandle};

/// A field is Plain HTML when HTML → Markdown → HTML, both sides normalized with html5ever, matches.
pub fn is_plain_html_field(html: &str) -> bool {
    let original = normalize_html(html);
    let Some(markdown) = html_to_markdown(html) else {
        return false;
    };
    let round_trip = markdown_to_html(&markdown);
    original == normalize_html(&round_trip)
}

pub fn html_to_markdown(html: &str) -> Option<String> {
    htmd::HtmlToMarkdown::new().convert(html).ok()
}

pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.render.unsafe_ = true;
    comrak::markdown_to_html(markdown, &options)
}

fn normalize_html(html: &str) -> String {
    let dom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .one(html.as_bytes());
    let mut out = Vec::new();
    let document: SerializableHandle = dom.document.clone().into();
    let opts = SerializeOpts {
        scripting_enabled: false,
        traversal_scope: html5ever::serialize::TraversalScope::ChildrenOnly(None),
        create_missing_parent: false,
    };
    if serialize(&mut out, &document, opts).is_err() {
        return html.to_string();
    }
    collapse_ws(&String::from_utf8_lossy(&out))
}

fn collapse_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            last_space = false;
            if ch.is_ascii_uppercase() {
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push(ch);
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paragraphs_are_editable() {
        let html = include_str!("../tests/fixtures/plain-paragraphs.html");
        assert!(is_plain_html_field(html), "paragraphs should round-trip");
    }

    #[test]
    fn plain_lists_are_editable() {
        let html = include_str!("../tests/fixtures/plain-lists.html");
        assert!(is_plain_html_field(html), "lists should round-trip");
    }

    #[test]
    fn mention_is_not_editable() {
        let html = include_str!("../tests/fixtures/mention.html");
        assert!(!is_plain_html_field(html), "@mention must not round-trip");
    }

    #[test]
    fn colored_span_is_not_editable() {
        let html = include_str!("../tests/fixtures/colored-span.html");
        assert!(
            !is_plain_html_field(html),
            "colored span must not round-trip"
        );
    }

    #[test]
    fn merged_cell_table_is_not_editable() {
        let html = include_str!("../tests/fixtures/merged-cell-table.html");
        assert!(
            !is_plain_html_field(html),
            "merged-cell table must not round-trip"
        );
    }
}
