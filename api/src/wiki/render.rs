//! Markdown → HTML pipeline. `pulldown-cmark` parses, then we hand off
//! to `ammonia` which strips dangerous tags/attributes per its
//! UGC-safe default policy.

use ammonia::Builder;
use pulldown_cmark::{html, Options, Parser};

pub struct RenderedPage {
    pub html: String,
    pub title: Option<String>,
}

pub fn render_markdown(md: &str) -> RenderedPage {
    let title = extract_title(md);

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(md, opts);
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    // Sanitize. Allow `class` on the common code elements so the
    // frontend can style language hints (`language-rust`, etc.) but
    // strip everything else by default.
    let safe = Builder::new()
        .add_tag_attributes("code", ["class"].iter().copied())
        .add_tag_attributes("pre", ["class"].iter().copied())
        .clean(&raw)
        .to_string();

    RenderedPage { html: safe, title }
}

/// Best-effort title from the first `# heading` line. Falls back to
/// `None` so the caller can use the slug if missing.
fn extract_title(md: &str) -> Option<String> {
    md.lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_and_paragraphs() {
        let out = render_markdown("# Hello\n\nWorld");
        assert!(out.html.contains("<h1>Hello</h1>"));
        assert!(out.html.contains("<p>World</p>"));
        assert_eq!(out.title.as_deref(), Some("Hello"));
    }

    #[test]
    fn strips_script_tags() {
        let out = render_markdown("<script>alert(1)</script>\n\nok");
        assert!(!out.html.contains("<script>"));
        assert!(out.html.contains("ok"));
    }

    #[test]
    fn keeps_code_class() {
        let out = render_markdown("```rust\nfn main(){}\n```");
        // pulldown-cmark emits <code class="language-rust">
        assert!(out.html.contains("language-rust"));
    }
}
