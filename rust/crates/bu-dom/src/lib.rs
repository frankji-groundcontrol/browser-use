//! Minimal DOM serialization primitives.

/// A small typed DOM node used by the first serializer slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomNode {
    tag: Option<String>,
    text: Option<String>,
    children: Vec<DomNode>,
}

impl DomNode {
    /// Creates an element node with the given tag name.
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: Some(tag.into()),
            text: None,
            children: Vec::new(),
        }
    }

    /// Creates a text-only node.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            tag: None,
            text: Some(text.into()),
            children: Vec::new(),
        }
    }

    /// Adds inline text to an element node.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Alias for [`DomNode::with_text`] that reads naturally in test fixtures.
    pub fn text_content(self, text: impl Into<String>) -> Self {
        self.with_text(text)
    }

    /// Adds a child node.
    pub fn child(mut self, child: DomNode) -> Self {
        self.children.push(child);
        self
    }
}

/// Serializes a typed DOM tree into the pseudo-HTML form consumed by agents.
pub fn serialize_dom(node: &DomNode) -> String {
    let mut serialized = String::new();
    serialize_node(node, &mut serialized);
    serialized
}

/// Character statistics for clean markdown extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownStats {
    /// Original HTML character count.
    pub original_html_chars: usize,
    /// Markdown character count before final whitespace filtering.
    pub initial_markdown_chars: usize,
    /// Final filtered markdown character count.
    pub final_filtered_chars: usize,
    /// Characters removed by final filtering.
    pub filtered_chars_removed: usize,
}

/// A structure-aware markdown chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownChunk {
    /// Chunk markdown content.
    pub content: String,
    /// Approximate character offset where this chunk starts.
    pub char_offset_start: usize,
    /// Approximate character offset where this chunk ends.
    pub char_offset_end: usize,
    /// Whether more chunks follow this one.
    pub has_more: bool,
    /// Optional context prefix for continuation chunks.
    pub overlap_prefix: Option<String>,
    /// Zero-based chunk index.
    pub chunk_index: usize,
    /// Total number of chunks.
    pub total_chunks: usize,
}

/// Extracts readable markdown from page HTML.
pub fn extract_clean_markdown(html: &str, extract_links: bool) -> (String, MarkdownStats) {
    let original_html_chars = html.chars().count();
    let mut markdown = String::new();
    let mut skip_stack = Vec::new();
    let mut link_stack = Vec::new();
    let mut cursor = 0;

    while let Some(tag_start) = html[cursor..].find('<') {
        let tag_start = cursor + tag_start;
        if skip_stack.is_empty() {
            append_text(&mut markdown, &html[cursor..tag_start]);
        }

        let Some(tag_end) = html[tag_start..].find('>') else {
            if skip_stack.is_empty() {
                append_text(&mut markdown, &html[tag_start..]);
            }
            cursor = html.len();
            break;
        };
        let tag_end = tag_start + tag_end;
        let tag = &html[tag_start + 1..tag_end];
        handle_tag(
            tag,
            extract_links,
            &mut markdown,
            &mut skip_stack,
            &mut link_stack,
        );
        cursor = tag_end + 1;
    }

    if cursor < html.len() && skip_stack.is_empty() {
        append_text(&mut markdown, &html[cursor..]);
    }

    let initial_markdown_chars = markdown.chars().count();
    let filtered = normalize_markdown(&markdown);
    let final_filtered_chars = filtered.chars().count();
    let filtered_chars_removed = initial_markdown_chars.saturating_sub(final_filtered_chars);

    (
        filtered,
        MarkdownStats {
            original_html_chars,
            initial_markdown_chars,
            final_filtered_chars,
            filtered_chars_removed,
        },
    )
}

/// Splits markdown into paragraph-oriented chunks.
pub fn chunk_markdown_by_structure(
    markdown: &str,
    max_chunk_chars: usize,
    start_from_char: usize,
) -> Vec<MarkdownChunk> {
    if max_chunk_chars == 0 || start_from_char >= markdown.chars().count() {
        return Vec::new();
    }

    let content = markdown.chars().skip(start_from_char).collect::<String>();
    let paragraphs = content.split("\n\n").collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_start = start_from_char;
    let mut offset = start_from_char;

    for paragraph in paragraphs {
        let separator = if current.is_empty() { "" } else { "\n\n" };
        let candidate_chars =
            current.chars().count() + separator.chars().count() + paragraph.chars().count();
        if !current.is_empty() && candidate_chars > max_chunk_chars {
            let end = offset.saturating_sub(2);
            chunks.push(MarkdownChunk {
                content: current.trim().to_owned(),
                char_offset_start: current_start,
                char_offset_end: end,
                has_more: true,
                overlap_prefix: None,
                chunk_index: 0,
                total_chunks: 0,
            });
            current = String::new();
            current_start = offset;
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(paragraph);
        offset += paragraph.chars().count() + 2;
    }

    if !current.trim().is_empty() {
        chunks.push(MarkdownChunk {
            char_offset_end: start_from_char + content.chars().count(),
            char_offset_start: current_start,
            content: current.trim().to_owned(),
            has_more: false,
            overlap_prefix: None,
            chunk_index: 0,
            total_chunks: 0,
        });
    }

    let total_chunks = chunks.len();
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = index;
        chunk.total_chunks = total_chunks;
        chunk.has_more = index + 1 < total_chunks;
    }

    chunks
}

fn serialize_node(node: &DomNode, out: &mut String) {
    if let Some(tag) = &node.tag {
        out.push('<');
        out.push_str(tag);
        out.push('>');
    }

    if let Some(text) = &node.text {
        out.push_str(text);
    }

    for child in &node.children {
        serialize_node(child, out);
    }

    if let Some(tag) = &node.tag {
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }
}

fn handle_tag(
    raw_tag: &str,
    extract_links: bool,
    markdown: &mut String,
    skip_stack: &mut Vec<String>,
    link_stack: &mut Vec<Option<String>>,
) {
    let tag = raw_tag.trim();
    if tag.is_empty() || tag.starts_with('!') || tag.starts_with('?') {
        return;
    }

    let closing = tag.starts_with('/');
    let tag_body = tag
        .trim_start_matches('/')
        .trim_start()
        .trim_end_matches('/')
        .trim();
    let name = tag_body
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if closing {
        if skip_stack.last() == Some(&name) {
            skip_stack.pop();
            return;
        }
        if !skip_stack.is_empty() {
            return;
        }
        match name.as_str() {
            "a" => {
                if let Some(Some(href)) = link_stack.pop() {
                    markdown.push_str("](");
                    markdown.push_str(&href);
                    markdown.push(')');
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "section" | "article"
            | "main" | "li" | "tr" => ensure_blank_line(markdown),
            _ => {}
        }
        return;
    }

    if matches!(name.as_str(), "script" | "style" | "nav") {
        skip_stack.push(name);
        return;
    }
    if !skip_stack.is_empty() {
        return;
    }

    match name.as_str() {
        "h1" => start_heading(markdown, 1),
        "h2" => start_heading(markdown, 2),
        "h3" => start_heading(markdown, 3),
        "h4" => start_heading(markdown, 4),
        "h5" => start_heading(markdown, 5),
        "h6" => start_heading(markdown, 6),
        "p" | "div" | "section" | "article" | "main" | "tr" => ensure_blank_line(markdown),
        "br" => markdown.push('\n'),
        "li" => {
            ensure_blank_line(markdown);
            markdown.push_str("- ");
        }
        "a" => {
            if extract_links {
                let href = attribute_value(tag_body, "href");
                if href.is_some() {
                    markdown.push('[');
                }
                link_stack.push(href);
            }
        }
        _ => {}
    }
}

fn start_heading(markdown: &mut String, level: usize) {
    ensure_blank_line(markdown);
    markdown.push_str(&"#".repeat(level));
    markdown.push(' ');
}

fn ensure_blank_line(markdown: &mut String) {
    let trimmed_len = markdown.trim_end().len();
    markdown.truncate(trimmed_len);
    if !markdown.is_empty() {
        markdown.push_str("\n\n");
    }
}

fn append_text(markdown: &mut String, text: &str) {
    let decoded = decode_html_entities(text);
    let compact = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return;
    }

    if markdown
        .chars()
        .last()
        .is_some_and(|last| !last.is_whitespace() && last != '[')
    {
        markdown.push(' ');
    }
    markdown.push_str(&compact);
}

fn normalize_markdown(markdown: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = true;
    for line in markdown.lines().map(str::trim) {
        if line.is_empty() {
            if !previous_blank {
                lines.push(String::new());
            }
            previous_blank = true;
        } else {
            lines.push(line.to_owned());
            previous_blank = false;
        }
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn attribute_value(tag_body: &str, attribute: &str) -> Option<String> {
    let lower = tag_body.to_ascii_lowercase();
    let attribute = attribute.to_ascii_lowercase();
    let start = lower.find(&attribute)?;
    let after_name = &tag_body[start + attribute.len()..];
    let after_equals = after_name.trim_start().strip_prefix('=')?.trim_start();
    let quote = after_equals.chars().next()?;
    if quote == '"' || quote == '\'' {
        let value = &after_equals[quote.len_utf8()..];
        return value.find(quote).map(|end| value[..end].to_owned());
    }
    Some(
        after_equals
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::{chunk_markdown_by_structure, extract_clean_markdown, serialize_dom, DomNode};

    #[test]
    fn serialize_smoke() {
        let serialized = serialize_dom(&DomNode::new("html"));

        assert!(!serialized.is_empty());
    }

    #[test]
    fn serializes_small_tree_as_pseudo_html() {
        let tree = DomNode::new("html").child(
            DomNode::new("body")
                .child(DomNode::text("Hello"))
                .child(DomNode::new("button").with_text("Go")),
        );

        assert_eq!(
            serialize_dom(&tree),
            "<html><body>Hello<button>Go</button></body></html>"
        );
    }

    #[test]
    fn clean_markdown_strips_noise_and_preserves_requested_links() {
        let (markdown, stats) = extract_clean_markdown(
            r#"
            <html>
              <head><style>body{color:red}</style><script>bad()</script></head>
              <body>
                <nav>Skip me</nav>
                <main><h1>Title</h1><p>Useful <strong>copy</strong></p><a href="/x">Details</a></main>
              </body>
            </html>
            "#,
            true,
        );

        assert!(markdown.contains("# Title"));
        assert!(markdown.contains("Useful copy"));
        assert!(markdown.contains("[Details](/x)"));
        assert!(!markdown.contains("Skip me"));
        assert!(!markdown.contains("bad()"));
        assert!(stats.original_html_chars > stats.final_filtered_chars);
        assert!(stats.filtered_chars_removed > 0);
    }

    #[test]
    fn chunker_splits_on_line_boundaries_and_reports_more_content() {
        let chunks = chunk_markdown_by_structure("alpha\n\nbeta\n\ngamma", 12, 0);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "alpha\n\nbeta");
        assert!(chunks[0].has_more);
        assert_eq!(chunks[1].content, "gamma");
        assert!(!chunks[1].has_more);
    }
}
