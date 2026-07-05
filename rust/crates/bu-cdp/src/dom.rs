//! Three-tree DOM fusion (DOM + DOMSnapshot + AX), interactive detection,
//! and paint-order / bounding-box filtering for the live selector map.

use std::collections::{HashMap, HashSet};

use chromiumoxide::cdp::browser_protocol::{
    accessibility::{AxNode, AxValue},
    dom::{BackendNodeId, Node},
    dom_snapshot::{ArrayOfStrings, CaptureSnapshotReturns, Rectangle, StringIndex},
};

use crate::geometry::{Rect, RectUnion, CONTAINMENT_THRESHOLD};
use crate::SelectorMapElement;

pub(crate) const REQUIRED_COMPUTED_STYLES: &[&str] = &[
    "display",
    "visibility",
    "opacity",
    "background-color",
    "cursor",
    "pointer-events",
];
#[derive(Debug, Clone, Default)]
pub(crate) struct EnhancedNode {
    pub(crate) backend_node_id: Option<BackendNodeId>,
    pub(crate) tag: String,
    pub(crate) attributes: HashMap<String, String>,
    pub(crate) text: String,
    pub(crate) ax_role: Option<String>,
    pub(crate) ax_name: Option<String>,
    pub(crate) ax_properties: HashMap<String, serde_json::Value>,
    pub(crate) computed_styles: HashMap<String, String>,
    pub(crate) bounds: Option<Rect>,
    pub(crate) paint_order: Option<i64>,
    pub(crate) has_js_click_listener: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct SelectorMapCandidate {
    backend_node_id: BackendNodeId,
    tag: String,
    text: String,
    href: Option<String>,
    bounds: Rect,
}

impl SelectorMapCandidate {
    fn backend_node_id_value(&self) -> i64 {
        *self.backend_node_id.inner()
    }

    pub(crate) fn into_element(self, index: usize) -> SelectorMapElement {
        SelectorMapElement {
            index,
            backend_node_id: self.backend_node_id,
            tag: self.tag,
            text: self.text,
            href: self.href,
            x: self.bounds.x + self.bounds.width / 2.0,
            y: self.bounds.y + self.bounds.height / 2.0,
        }
    }
}
pub(crate) fn collect_enhanced_dom_nodes(node: &Node, nodes: &mut HashMap<i64, EnhancedNode>) {
    let backend_node_id = *node.backend_node_id.inner();
    let tag = node_tag(node);
    let attributes = node_attributes(node);
    let text = if node.node_type == 1 {
        short_text(node_label(node))
    } else if node.node_type == 3 {
        short_text(node.node_value.trim().to_owned())
    } else {
        String::new()
    };

    let enhanced = nodes.entry(backend_node_id).or_default();
    enhanced.backend_node_id = Some(node.backend_node_id);
    if !tag.is_empty() {
        enhanced.tag = tag;
    }
    if !attributes.is_empty() {
        enhanced.attributes = attributes;
    }
    if !text.is_empty() {
        enhanced.text = text;
    }

    for child in node.children.iter().flatten() {
        collect_enhanced_dom_nodes(child, nodes);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        collect_enhanced_dom_nodes(shadow_root, nodes);
    }
    if let Some(content_document) = &node.content_document {
        collect_enhanced_dom_nodes(content_document, nodes);
    }
    if let Some(template_content) = &node.template_content {
        collect_enhanced_dom_nodes(template_content, nodes);
    }
}

pub(crate) fn merge_snapshot(
    snapshot: &CaptureSnapshotReturns,
    fallback_scroll_x: f64,
    fallback_scroll_y: f64,
    nodes: &mut HashMap<i64, EnhancedNode>,
) {
    for document in &snapshot.documents {
        // DOMSnapshot often omits (or zeroes) the per-document scroll offset; fall
        // back to the live main-frame scroll so bounds normalize to the viewport —
        // otherwise a scrolled-into-view element is wrongly filtered as off-screen.
        let scroll_x = document
            .scroll_offset_x
            .filter(|value| *value != 0.0)
            .unwrap_or(fallback_scroll_x);
        let scroll_y = document
            .scroll_offset_y
            .filter(|value| *value != 0.0)
            .unwrap_or(fallback_scroll_y);

        let Some(backend_node_ids) = &document.nodes.backend_node_id else {
            continue;
        };

        for (node_index, backend_node_id) in backend_node_ids.iter().enumerate() {
            let backend_node_id_value = *backend_node_id.inner();
            let enhanced = nodes.entry(backend_node_id_value).or_default();
            enhanced.backend_node_id = Some(*backend_node_id);

            if enhanced.tag.is_empty() {
                if let Some(tag) = document
                    .nodes
                    .node_name
                    .as_ref()
                    .and_then(|names| names.get(node_index))
                    .and_then(|index| snapshot_string(snapshot, *index))
                {
                    enhanced.tag = tag.to_ascii_lowercase();
                }
            }

            if enhanced.text.is_empty() {
                if let Some(text) = document
                    .nodes
                    .node_value
                    .as_ref()
                    .and_then(|values| values.get(node_index))
                    .and_then(|index| snapshot_string(snapshot, *index))
                    .filter(|text| !text.trim().is_empty())
                {
                    enhanced.text = short_text(text.trim().to_owned());
                }
            }

            if enhanced.attributes.is_empty() {
                if let Some(attributes) = document
                    .nodes
                    .attributes
                    .as_ref()
                    .and_then(|attributes| attributes.get(node_index))
                {
                    enhanced.attributes = snapshot_attributes(snapshot, attributes);
                }
            }
        }

        for (layout_index, node_index) in document.layout.node_index.iter().enumerate() {
            let Ok(node_index) = usize::try_from(*node_index) else {
                continue;
            };
            let Some(backend_node_id) = backend_node_ids.get(node_index) else {
                continue;
            };

            let enhanced = nodes.entry(*backend_node_id.inner()).or_default();
            enhanced.backend_node_id = Some(*backend_node_id);

            if let Some(styles) = document.layout.styles.get(layout_index) {
                enhanced.computed_styles = snapshot_computed_styles(snapshot, styles);
            }

            if let Some(bounds) = document
                .layout
                .bounds
                .get(layout_index)
                .and_then(rect_from_snapshot)
            {
                enhanced.bounds = Some(Rect {
                    x: bounds.x - scroll_x,
                    y: bounds.y - scroll_y,
                    width: bounds.width,
                    height: bounds.height,
                });
            }

            if let Some(paint_order) = document
                .layout
                .paint_orders
                .as_ref()
                .and_then(|paint_orders| paint_orders.get(layout_index))
            {
                enhanced.paint_order = Some(*paint_order);
            }

            if enhanced.text.is_empty() {
                if let Some(text) = document
                    .layout
                    .text
                    .get(layout_index)
                    .and_then(|index| snapshot_string(snapshot, *index))
                    .filter(|text| !text.trim().is_empty())
                {
                    enhanced.text = short_text(text.trim().to_owned());
                }
            }
        }
    }
}

pub(crate) fn merge_ax_tree(ax_nodes: &[AxNode], nodes: &mut HashMap<i64, EnhancedNode>) {
    for ax_node in ax_nodes {
        let Some(backend_node_id) = ax_node.backend_dom_node_id else {
            continue;
        };

        let enhanced = nodes.entry(*backend_node_id.inner()).or_default();
        enhanced.backend_node_id = Some(backend_node_id);
        enhanced.ax_role = ax_node.role.as_ref().and_then(ax_value_string);
        enhanced.ax_name = ax_node.name.as_ref().and_then(ax_value_string);

        if enhanced.text.is_empty() {
            if let Some(name) = &enhanced.ax_name {
                enhanced.text = short_text(name.clone());
            }
        }

        if let Some(properties) = &ax_node.properties {
            enhanced.ax_properties = properties
                .iter()
                .map(|property| {
                    (
                        property.name.as_ref().to_owned(),
                        property
                            .value
                            .value
                            .clone()
                            .unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect();
        }
    }
}

pub(crate) fn visible_backend_node_ids(nodes: &HashMap<i64, EnhancedNode>) -> Vec<BackendNodeId> {
    nodes
        .values()
        .filter(|node| is_visible_enhanced_node(node))
        .filter_map(|node| node.backend_node_id)
        .collect()
}

pub(crate) fn collect_interactive_elements(
    node: &Node,
    enhanced_nodes: &HashMap<i64, EnhancedNode>,
    elements: &mut Vec<SelectorMapCandidate>,
) {
    if node.node_type == 1 {
        if let Some(enhanced) = enhanced_nodes.get(node.backend_node_id.inner()) {
            if is_visible_enhanced_node(enhanced) && is_interactive_enhanced_node(enhanced) {
                if let (Some(backend_node_id), Some(bounds)) =
                    (enhanced.backend_node_id, enhanced.bounds)
                {
                    elements.push(SelectorMapCandidate {
                        backend_node_id,
                        tag: enhanced.tag.clone(),
                        text: enhanced.text.clone(),
                        href: enhanced.attributes.get("href").cloned(),
                        bounds,
                    });
                }
            }
        }
    }

    for child in node.children.iter().flatten() {
        collect_interactive_elements(child, enhanced_nodes, elements);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        collect_interactive_elements(shadow_root, enhanced_nodes, elements);
    }
    if let Some(content_document) = &node.content_document {
        collect_interactive_elements(content_document, enhanced_nodes, elements);
    }
    if let Some(template_content) = &node.template_content {
        collect_interactive_elements(template_content, enhanced_nodes, elements);
    }
}

/// Drops interactive candidates fully covered by higher-painted opaque elements
/// (e.g. a button under a full-screen modal backdrop). Mirrors Python's PaintOrderRemover.
pub(crate) fn apply_paint_order_occlusion_filter(
    candidates: &mut Vec<SelectorMapCandidate>,
    enhanced: &HashMap<i64, EnhancedNode>,
) {
    // Opaque occluders sorted front-to-back (highest paint order first).
    let mut occluders: Vec<(i64, Rect)> = enhanced
        .values()
        .filter(|node| is_opaque_enhanced_node(node))
        .filter_map(|node| Some((node.paint_order?, node.bounds?)))
        .filter(|(_, rect)| !rect.is_empty())
        .collect();
    occluders.sort_by(|a, b| b.0.cmp(&a.0));

    candidates.retain(|candidate| {
        let candidate_paint = enhanced
            .get(&candidate.backend_node_id_value())
            .and_then(|node| node.paint_order)
            .unwrap_or(i64::MIN);
        let mut union = RectUnion::default();
        for (paint_order, rect) in &occluders {
            // Only elements painted strictly above the candidate can occlude it.
            if *paint_order <= candidate_paint {
                break;
            }
            union.add(*rect);
            if union.contains(candidate.bounds) {
                return false;
            }
        }
        true
    });
}

/// Collapses a candidate that is >=99% contained inside a propagating interactive
/// parent (a/button/[role=button|link|combobox]) into that parent — a button
/// wrapping icon+text yields one index. Mirrors Python's _apply_bounding_box_filtering.
/// Collapses candidates contained in a propagating interactive ANCESTOR into
/// that ancestor. Mirrors Python's `_apply_bounding_box_filtering`: bounds
/// propagate DOWN the DOM tree (ancestor -> descendant only), never across
/// siblings, so a stretched-link sibling cannot drop a sibling button.
pub(crate) fn apply_bounding_box_containment_filter(
    dom_tree: &Node,
    enhanced: &HashMap<i64, EnhancedNode>,
    candidates: &mut Vec<SelectorMapCandidate>,
) {
    let mut excluded: HashSet<i64> = HashSet::new();
    filter_tree_recursive(dom_tree, None, enhanced, &mut excluded);
    candidates.retain(|candidate| !excluded.contains(&candidate.backend_node_id_value()));
}

fn filter_tree_recursive(
    node: &Node,
    active_bounds: Option<Rect>,
    enhanced: &HashMap<i64, EnhancedNode>,
    excluded: &mut HashSet<i64>,
) {
    if node.node_type == 1 {
        let backend_node_id = *node.backend_node_id.inner();
        if let Some(bounds) = active_bounds {
            if should_exclude_child(backend_node_id, enhanced, bounds) {
                excluded.insert(backend_node_id);
            }
        }
    }

    // A propagating element starts new bounds for its whole subtree (even if it
    // was itself excluded).
    let mut new_bounds = None;
    if node.node_type == 1 {
        if let Some(enhanced_node) = enhanced.get(node.backend_node_id.inner()) {
            if is_propagating_element(enhanced_node) {
                new_bounds = enhanced_node.bounds;
            }
        }
    }
    let propagate = new_bounds.or(active_bounds);

    for child in node.children.iter().flatten() {
        filter_tree_recursive(child, propagate, enhanced, excluded);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        filter_tree_recursive(shadow_root, propagate, enhanced, excluded);
    }
    if let Some(content_document) = &node.content_document {
        filter_tree_recursive(content_document, propagate, enhanced, excluded);
    }
    if let Some(template_content) = &node.template_content {
        filter_tree_recursive(template_content, propagate, enhanced, excluded);
    }
}

/// Mirrors Python's `_should_exclude_child`: exclude a contained descendant
/// unless it independently warrants its own index.
fn should_exclude_child(
    backend_node_id: i64,
    enhanced: &HashMap<i64, EnhancedNode>,
    parent_bounds: Rect,
) -> bool {
    let Some(node) = enhanced.get(&backend_node_id) else {
        return false;
    };
    let Some(child_bounds) = node.bounds else {
        return false;
    };
    if child_bounds.area() <= 0.0 {
        return false;
    }
    if parent_bounds.intersection_area(child_bounds) / child_bounds.area() < CONTAINMENT_THRESHOLD {
        return false;
    }

    // Exception rules — keep the child even though it is contained:
    if matches!(node.tag.as_str(), "input" | "select" | "textarea" | "label") {
        return false;
    }
    if is_propagating_element(node) {
        return false;
    }
    if node.attributes.contains_key("onclick") {
        return false;
    }
    if node
        .attributes
        .get("aria-label")
        .is_some_and(|value| !value.trim().is_empty())
    {
        return false;
    }
    if node.attributes.get("role").is_some_and(|role| {
        matches!(
            role.as_str(),
            "button" | "link" | "checkbox" | "radio" | "tab" | "menuitem" | "option"
        )
    }) {
        return false;
    }

    true
}

/// Mirrors Python's PROPAGATING_ELEMENTS + `_is_propagating_element`. Uses the
/// `role` ATTRIBUTE only (not the AX role); `role=link` and `<select>` are
/// intentionally NOT propagating.
fn is_propagating_element(node: &EnhancedNode) -> bool {
    let role = node.attributes.get("role").map(String::as_str);
    matches!(
        (node.tag.as_str(), role),
        ("a", _)
            | ("button", _)
            | ("div", Some("button"))
            | ("div", Some("combobox"))
            | ("span", Some("button"))
            | ("span", Some("combobox"))
            | ("input", Some("combobox"))
    )
}

/// An element occludes what is behind it iff its background is not fully
/// transparent AND opacity >= 0.8 — mirrors Python's PaintOrderRemover, which
/// skips occluders whose background is exactly `rgba(0, 0, 0, 0)` or opacity < 0.8.
fn is_opaque_enhanced_node(node: &EnhancedNode) -> bool {
    let opacity = node
        .computed_styles
        .get("opacity")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(1.0);
    if opacity < 0.8 {
        return false;
    }
    node.computed_styles
        .get("background-color")
        .is_some_and(|background| background != "rgba(0, 0, 0, 0)")
}

pub(crate) fn is_click_like_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "click" | "mousedown" | "mouseup" | "pointerdown" | "pointerup"
    )
}

fn is_interactive_enhanced_node(node: &EnhancedNode) -> bool {
    node.has_js_click_listener
        || node.ax_role.as_deref().is_some_and(is_interactive_ax_role)
        || is_interactive_tag(&node.tag)
        || node.attributes.contains_key("onclick")
        || node.attributes.contains_key("contenteditable")
        || node
            .attributes
            .get("tabindex")
            .and_then(|value| value.trim().parse::<i64>().ok())
            .is_some_and(|tabindex| tabindex >= 0)
}

fn is_visible_enhanced_node(node: &EnhancedNode) -> bool {
    if node
        .computed_styles
        .get("display")
        .is_some_and(|display| display.eq_ignore_ascii_case("none"))
    {
        return false;
    }

    if node
        .computed_styles
        .get("visibility")
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("hidden"))
    {
        return false;
    }

    if node
        .computed_styles
        .get("opacity")
        .and_then(|opacity| opacity.parse::<f64>().ok())
        .is_some_and(|opacity| opacity <= 0.0)
    {
        return false;
    }

    node.bounds
        .is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
}

fn is_interactive_ax_role(role: &str) -> bool {
    matches!(
        role.to_ascii_lowercase().as_str(),
        "link"
            | "button"
            | "menuitem"
            | "option"
            | "radio"
            | "checkbox"
            | "tab"
            | "textbox"
            | "combobox"
            | "slider"
            | "spinbutton"
            | "listbox"
            | "switch"
    )
}

fn is_interactive_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "button" | "input" | "select" | "textarea" | "summary" | "label"
    )
}

fn snapshot_string(snapshot: &CaptureSnapshotReturns, index: StringIndex) -> Option<&str> {
    let index = usize::try_from(*index.inner()).ok()?;
    snapshot.strings.get(index).map(String::as_str)
}

fn snapshot_attributes(
    snapshot: &CaptureSnapshotReturns,
    attributes: &ArrayOfStrings,
) -> HashMap<String, String> {
    attributes
        .inner()
        .chunks_exact(2)
        .filter_map(|chunk| {
            let name = snapshot_string(snapshot, chunk[0])?.to_ascii_lowercase();
            let value = snapshot_string(snapshot, chunk[1])?.to_owned();
            Some((name, value))
        })
        .collect()
}

fn snapshot_computed_styles(
    snapshot: &CaptureSnapshotReturns,
    styles: &ArrayOfStrings,
) -> HashMap<String, String> {
    REQUIRED_COMPUTED_STYLES
        .iter()
        .zip(styles.inner())
        .filter_map(|(name, value_index)| {
            Some((
                (*name).to_owned(),
                snapshot_string(snapshot, *value_index)?.to_owned(),
            ))
        })
        .collect()
}

fn rect_from_snapshot(rectangle: &Rectangle) -> Option<Rect> {
    let values = rectangle.inner();
    Some(Rect {
        x: *values.first()?,
        y: *values.get(1)?,
        width: *values.get(2)?,
        height: *values.get(3)?,
    })
}

fn ax_value_string(value: &AxValue) -> Option<String> {
    match value.value.as_ref()? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn node_tag(node: &Node) -> String {
    let tag = if node.local_name.is_empty() {
        &node.node_name
    } else {
        &node.local_name
    };
    tag.to_ascii_lowercase()
}

fn node_attributes(node: &Node) -> HashMap<String, String> {
    let Some(attributes) = &node.attributes else {
        return HashMap::new();
    };

    attributes
        .chunks_exact(2)
        .map(|chunk| (chunk[0].to_ascii_lowercase(), chunk[1].clone()))
        .collect()
}

fn node_label(node: &Node) -> String {
    ["aria-label", "title", "placeholder", "value", "alt"]
        .into_iter()
        .filter_map(|name| attr_value(node, name))
        .find(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| descendant_text(node))
}

fn descendant_text(node: &Node) -> String {
    let mut parts = Vec::new();
    collect_text(node, &mut parts);
    parts.join(" ")
}

fn collect_text(node: &Node, parts: &mut Vec<String>) {
    if node.node_type == 3 {
        let text = node.node_value.trim();
        if !text.is_empty() {
            parts.push(text.to_owned());
        }
    }

    for child in node.children.iter().flatten() {
        collect_text(child, parts);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        collect_text(shadow_root, parts);
    }
}

fn short_text(text: String) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 80;
    if compact.chars().count() <= MAX_CHARS {
        return compact;
    }

    compact.chars().take(MAX_CHARS).collect()
}

fn attr_value<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attributes.as_ref()?.chunks_exact(2).find_map(|chunk| {
        chunk[0]
            .eq_ignore_ascii_case(name)
            .then_some(chunk[1].as_str())
    })
}
