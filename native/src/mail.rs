//! Imported-email rendering.
//!
//! The web app has a real browser engine, so it can show the original HTML.
//! This module is an honest substitute: a small tag reader that keeps the
//! structure (headings, paragraphs, quotes, lists, line breaks), inline
//! emphasis (`b/i/code`), links and entity references, and drops everything a
//! native renderer cannot honour (CSS, tables-as-layout, images, scripts).
//!
//! Nothing here writes to disk: imported emails stay read-only.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

impl Span {
    fn plain(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MailBlock {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Quote(Vec<Span>),
    Bullet(Vec<Span>),
    Rule,
}

impl MailBlock {
    pub fn spans(&self) -> &[Span] {
        match self {
            MailBlock::Heading(_, spans)
            | MailBlock::Paragraph(spans)
            | MailBlock::Quote(spans)
            | MailBlock::Bullet(spans) => spans,
            MailBlock::Rule => &[],
        }
    }

    pub fn text(&self) -> String {
        self.spans()
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>()
    }
}

#[derive(Clone, Copy, Default)]
struct BlockKind {
    heading: Option<u8>,
    quote: bool,
    bullet: bool,
}

struct Reader {
    blocks: Vec<MailBlock>,
    spans: Vec<Span>,
    buffer: String,
    style: Span,
    block_stack: Vec<BlockKind>,
    kind: BlockKind,
}

impl Reader {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            spans: Vec::new(),
            buffer: String::new(),
            style: Span::default(),
            block_stack: Vec::new(),
            kind: BlockKind::default(),
        }
    }

    fn flush_text(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = collapse_spaces(&self.buffer);
        self.buffer.clear();
        if text.is_empty() {
            return;
        }
        let span = Span {
            text,
            ..self.style.clone()
        };
        self.spans.push(span);
    }

    fn flush_block(&mut self) {
        self.flush_text();
        // Inline spans keep their boundary spaces so `**bold** word` reads
        // naturally; trim only at the edges of the finished block.
        if let Some(first) = self.spans.first_mut() {
            first.text = first.text.trim_start().to_string();
        }
        if let Some(last) = self.spans.last_mut() {
            last.text = last.text.trim_end().to_string();
        }
        self.spans.retain(|span| !span.text.is_empty());
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        let block = match self.kind.heading {
            Some(level) => MailBlock::Heading(level.clamp(1, 3), spans),
            None if self.kind.quote => MailBlock::Quote(spans),
            None if self.kind.bullet => MailBlock::Bullet(spans),
            None => MailBlock::Paragraph(spans),
        };
        self.blocks.push(block);
    }

    fn open(&mut self, tag: &str, attrs: &str) {
        let name = tag.to_ascii_lowercase();
        match name.as_str() {
            "b" | "strong" => self.style.bold = true,
            "i" | "em" => self.style.italic = true,
            "code" | "tt" | "kbd" | "pre" => self.style.code = true,
            "a" => self.style.link = href_of(attrs),
            "hr" => {
                self.flush_block();
                self.blocks.push(MailBlock::Rule);
            }
            _ if !is_block_tag(&name) => {}
            _ => {
                self.flush_block();
                self.block_stack.push(self.kind);
                match name.as_str() {
                    "h1" => self.kind.heading = Some(1),
                    "h2" => self.kind.heading = Some(2),
                    "h3" | "h4" | "h5" | "h6" => self.kind.heading = Some(3),
                    "blockquote" => self.kind.quote = true,
                    "li" => {
                        self.kind.bullet = true;
                        self.kind.heading = None;
                    }
                    _ => {}
                }
            }
        }
    }

    fn close(&mut self, tag: &str) {
        let name = tag.to_ascii_lowercase();
        match name.as_str() {
            "b" | "strong" => self.style.bold = false,
            "i" | "em" => self.style.italic = false,
            "code" | "tt" | "kbd" | "pre" => self.style.code = false,
            "a" => self.style.link = None,
            _ if !is_block_tag(&name) => {}
            _ => {
                self.flush_block();
                self.kind = self.block_stack.pop().unwrap_or_default();
            }
        }
    }

    fn line_break(&mut self) {
        self.flush_block();
    }
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "tr"
            | "td"
            | "li"
            | "ul"
            | "ol"
            | "blockquote"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "table"
            | "tbody"
            | "thead"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "center"
            | "pre"
    )
}

fn href_of(attrs: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let index = lower.find("href")?;
    let rest = attrs[index + 4..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let value = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next()?
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        stripped.split('\'').next()?
    } else {
        rest.split_whitespace().next()?
    };
    let value = decode_entities(value);
    let scheme_ok = value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("mailto:");
    scheme_ok.then_some(value)
}

/// Remove `<style>`, `<script>` and comments before any other processing.
fn strip_invisible(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        let lowered = rest.to_ascii_lowercase();
        let next = ["<style", "<script", "<!--"]
            .iter()
            .filter_map(|needle| lowered.find(needle).map(|index| (index, *needle)))
            .min();
        let Some((index, needle)) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        let close = match needle {
            "<!--" => "-->".to_string(),
            "<style" => "</style>".to_string(),
            _ => "</script>".to_string(),
        };
        match lowered[index..].find(&close) {
            Some(offset) => {
                rest = &rest[index + offset + close.len()..];
            }
            None => break,
        }
    }
    out
}

/// Parse an HTML part into display blocks.
pub fn html_to_blocks(html: &str) -> Vec<MailBlock> {
    let html = strip_invisible(html);
    let mut reader = Reader::new();
    let bytes = html.as_bytes();

    let mut index = 0usize;
    let mut text_start = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'<' {
            reader.buffer.push_str(&html[text_start..index]);
            reader.flush_text();
            let Some(close) = html[index..].find('>') else {
                break;
            };
            let raw_tag = html[index + 1..index + close].trim();
            if let Some(name) = raw_tag.strip_prefix('/') {
                let name = name
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("");
                reader.close(name);
            } else if !raw_tag.starts_with('!') && !raw_tag.starts_with('?') {
                let name = raw_tag
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("");
                let attrs = &raw_tag[name.len()..];
                if name.eq_ignore_ascii_case("br") {
                    reader.line_break();
                } else {
                    reader.open(name, attrs);
                }
            }
            index += close + 1;
            text_start = index;
        } else {
            index += 1;
        }
    }
    reader.buffer.push_str(&html[text_start..]);
    reader.flush_block();
    reader.blocks
}

/// Plain-text variant, used for previews and the search index.
pub fn html_to_text(html: &str) -> String {
    html_to_blocks(html)
        .iter()
        .map(MailBlock::text)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn plain_to_blocks(text: &str) -> Vec<MailBlock> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .map(|paragraph| {
            let mut spans = Vec::new();
            for (index, line) in paragraph.split('\n').enumerate() {
                if index > 0 {
                    spans.push(Span::plain("\n".to_string()));
                }
                if !line.trim().is_empty() {
                    spans.push(Span::plain(line.trim_end().to_string()));
                }
            }
            MailBlock::Paragraph(spans)
        })
        .filter(|block| !block.text().trim().is_empty())
        .collect()
}

fn collapse_spaces(text: &str) -> String {
    let decoded = decode_entities(text);
    let mut out = String::with_capacity(decoded.len());
    let mut pending_space = false;
    for ch in decoded.chars() {
        if matches!(
            ch,
            '\u{034f}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}'
        ) {
            continue;
        }
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    if pending_space {
        out.push(' ');
    }
    out
}

pub fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            "hellip" => Some('…'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "trade" => Some('™'),
            "euro" => Some('€'),
            "pound" => Some('£'),
            "times" => Some('×'),
            "middot" => Some('·'),
            "bull" => Some('•'),
            _ => None,
        };
        let decoded = decoded.or_else(|| {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = entity.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        });
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_structure_and_inline_emphasis() {
        let html = "<h1>Invoice</h1><p>Total: <strong>42 €</strong> due.</p><ul><li>one</li><li>two</li></ul>";
        let blocks = html_to_blocks(html);
        assert!(matches!(blocks[0], MailBlock::Heading(1, _)));
        assert_eq!(blocks[1].text(), "Total: 42 € due.");
        assert!(blocks[1]
            .spans()
            .iter()
            .any(|span| span.bold && span.text == "42 €"));
        assert_eq!(
            blocks[2],
            MailBlock::Bullet(vec![Span::plain("one".into())])
        );
        assert_eq!(blocks[3].text(), "two");
    }

    #[test]
    fn decodes_entities_and_drops_zero_width_padding() {
        assert_eq!(decode_entities("a&amp;b &#8212; c"), "a&b — c");
        assert_eq!(
            collapse_spaces("a&amp;b &#8212; c&nbsp;&#x200B;d"),
            "a&b — c d"
        );
    }

    #[test]
    fn strips_scripts_and_styles() {
        let html = "<style>p{color:red}</style><script>alert(1)</script><p>Hi</p>";
        let text = html_to_text(html);
        assert_eq!(text, "Hi");
    }

    #[test]
    fn keeps_links_but_only_safe_schemes() {
        let blocks = html_to_blocks(
            "<p><a href=\"https://example.com\">site</a> <a href=\"javascript:evil\">bad</a></p>",
        );
        let spans = blocks[0].spans();
        assert_eq!(spans[0].link.as_deref(), Some("https://example.com"));
        assert!(spans
            .iter()
            .any(|span| span.text.contains("bad") && span.link.is_none()));
    }

    #[test]
    fn plain_text_becomes_paragraphs() {
        let blocks = plain_to_blocks("first line\nsecond\n\nnext paragraph");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text(), "first line\nsecond");
    }
}
