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

#[cfg(test)]
mod tests {
    use super::{serialize_dom, DomNode};

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
}
