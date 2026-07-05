//! DOM serialization placeholder.

/// Serializes a browser DOM snapshot into the pseudo-HTML form consumed by agents.
pub fn serialize_dom(_snapshot: &str) -> String {
    unimplemented!("DOM serialization is not implemented yet")
}

#[cfg(test)]
mod tests {
    use super::serialize_dom;

    #[test]
    fn serialize_smoke() {
        let serialized = serialize_dom("<html><body>Hello</body></html>");

        assert!(!serialized.is_empty());
    }
}
