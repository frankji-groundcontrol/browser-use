//! Rectangle geometry + disjoint-rect union for DOM occlusion/containment filtering.

const MAX_OCCLUSION_RECTS: usize = 5_000;
pub(crate) const CONTAINMENT_THRESHOLD: f64 = 0.99;
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Rect {
    pub(crate) fn x2(self) -> f64 {
        self.x + self.width
    }

    pub(crate) fn y2(self) -> f64 {
        self.y + self.height
    }

    pub(crate) fn area(self) -> f64 {
        self.width * self.height
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub(crate) fn intersects(self, other: Rect) -> bool {
        !(self.x2() <= other.x
            || other.x2() <= self.x
            || self.y2() <= other.y
            || other.y2() <= self.y)
    }

    pub(crate) fn contains(self, other: Rect) -> bool {
        self.x <= other.x && self.y <= other.y && self.x2() >= other.x2() && self.y2() >= other.y2()
    }

    pub(crate) fn intersection_area(self, other: Rect) -> f64 {
        let x_overlap = (self.x2().min(other.x2()) - self.x.max(other.x)).max(0.0);
        let y_overlap = (self.y2().min(other.y2()) - self.y.max(other.y)).max(0.0);
        x_overlap * y_overlap
    }

    pub(crate) fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Whether a compositor mouse event at this box's centre would miss it.
    ///
    /// The click lands on the centre, so that point must be inside the viewport
    /// (`viewport` is `(width, height)` in CSS px); testing the box's own edges
    /// instead would divert a tall, partly scrolled element to a synthetic
    /// `click()` that pointer-driven widgets ignore. A box under 2×2 px is also
    /// sent to the DOM. Flutter Web's `flt-semantics-placeholder` is 1×1 CSS px at
    /// `(-1, -1)`: a mouse event at that centre misses the node, and
    /// `HTMLElement.click()` does not.
    pub(crate) fn prefers_dom_click(self, viewport: (f64, f64)) -> bool {
        const MIN_MOUSE_EDGE: f64 = 2.0;
        let (cx, cy) = self.center();
        (self.width < MIN_MOUSE_EDGE && self.height < MIN_MOUSE_EDGE)
            || cx < 0.0
            || cy < 0.0
            || cx >= viewport.0
            || cy >= viewport.1
    }

    /// Bounding box of a CDP content quad `[x1,y1, x2,y2, x3,y3, x4,y4]`.
    pub(crate) fn from_quad_points(points: &[f64]) -> Option<Self> {
        if points.len() < 8 {
            return None;
        }
        let xs = [points[0], points[2], points[4], points[6]];
        let ys = [points[1], points[3], points[5], points[7]];
        let min_x = xs.into_iter().fold(f64::INFINITY, f64::min);
        let max_x = xs.into_iter().fold(f64::NEG_INFINITY, f64::max);
        let min_y = ys.into_iter().fold(f64::INFINITY, f64::min);
        let max_y = ys.into_iter().fold(f64::NEG_INFINITY, f64::max);
        if !min_x.is_finite() || !min_y.is_finite() {
            return None;
        }
        Some(Self {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        })
    }
}
#[derive(Debug, Default)]
pub(crate) struct RectUnion {
    rects: Vec<Rect>,
}

impl RectUnion {
    pub(crate) fn contains(&self, rect: Rect) -> bool {
        if self.rects.is_empty() || rect.is_empty() {
            return false;
        }

        let mut pending = vec![rect];
        for covered in &self.rects {
            let mut next_pending = Vec::new();
            for piece in pending {
                if covered.contains(piece) {
                    continue;
                }
                if piece.intersects(*covered) {
                    next_pending.extend(split_rect_difference(piece, *covered));
                } else {
                    next_pending.push(piece);
                }
            }
            if next_pending.is_empty() {
                return true;
            }
            pending = next_pending;
        }

        false
    }

    pub(crate) fn add(&mut self, rect: Rect) -> bool {
        if rect.is_empty() || self.rects.len() >= MAX_OCCLUSION_RECTS || self.contains(rect) {
            return false;
        }

        let mut pending = vec![rect];
        for existing in &self.rects {
            let mut next_pending = Vec::new();
            for piece in pending {
                if piece.intersects(*existing) {
                    next_pending.extend(split_rect_difference(piece, *existing));
                } else {
                    next_pending.push(piece);
                }
            }
            pending = next_pending;
            if pending.is_empty() {
                return false;
            }
        }

        // The cap is enforced only at entry (matching Python's RectUnionPure);
        // a post-subtraction guard would drop a legitimate occluder near the cap
        // and cause over-inclusion of elements it covers.
        self.rects.extend(pending);
        true
    }
}

fn split_rect_difference(rect: Rect, cutter: Rect) -> Vec<Rect> {
    let mut parts = Vec::with_capacity(4);

    if rect.y < cutter.y {
        parts.push(Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: cutter.y - rect.y,
        });
    }
    if cutter.y2() < rect.y2() {
        parts.push(Rect {
            x: rect.x,
            y: cutter.y2(),
            width: rect.width,
            height: rect.y2() - cutter.y2(),
        });
    }

    let y = rect.y.max(cutter.y);
    let y2 = rect.y2().min(cutter.y2());
    let height = y2 - y;

    if rect.x < cutter.x && height > 0.0 {
        parts.push(Rect {
            x: rect.x,
            y,
            width: cutter.x - rect.x,
            height,
        });
    }
    if cutter.x2() < rect.x2() && height > 0.0 {
        parts.push(Rect {
            x: cutter.x2(),
            y,
            width: rect.x2() - cutter.x2(),
            height,
        });
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::Rect;

    const VIEWPORT: (f64, f64) = (1280.0, 800.0);

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn flutter_a11y_placeholder_prefers_dom_click() {
        let placeholder = rect(-1.0, -1.0, 1.0, 1.0);
        assert!(placeholder.prefers_dom_click(VIEWPORT));
        let (cx, cy) = placeholder.center();
        assert!((cx + 0.5).abs() < f64::EPSILON);
        assert!((cy + 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn in_viewport_form_controls_keep_mouse_click() {
        assert!(!rect(46.76, 288.06, 296.48, 20.0).prefers_dom_click(VIEWPORT));
        assert!(!rect(16.0, 790.0, 358.0, 6.0).prefers_dom_click(VIEWPORT));
    }

    #[test]
    fn partly_clipped_element_with_an_on_screen_centre_keeps_mouse_click() {
        // A tall menu item scrolled 30px past the top. Its top-left is negative but
        // the point a mouse would hit is visible, and a trusted event is what
        // pointer-driven widgets (Radix, react-select) listen for.
        assert!(!rect(100.0, -30.0, 200.0, 400.0).prefers_dom_click(VIEWPORT));
        assert!(!rect(-40.0, 100.0, 400.0, 50.0).prefers_dom_click(VIEWPORT));
    }

    #[test]
    fn element_whose_centre_is_off_screen_prefers_dom_click() {
        // Past the bottom and right edges, which the old left/top-only test missed.
        assert!(rect(100.0, 790.0, 200.0, 200.0).prefers_dom_click(VIEWPORT));
        assert!(rect(1270.0, 100.0, 200.0, 40.0).prefers_dom_click(VIEWPORT));
        // Wholly above the viewport.
        assert!(rect(100.0, -300.0, 200.0, 100.0).prefers_dom_click(VIEWPORT));
        // The viewport is half-open: a centre exactly on the far edge is outside.
        assert!(rect(0.0, 700.0, 1280.0, 200.0).prefers_dom_click(VIEWPORT));
    }

    #[test]
    fn unknown_viewport_only_rejects_negative_centres() {
        let unknown = (f64::INFINITY, f64::INFINITY);
        assert!(!rect(100.0, 5000.0, 200.0, 40.0).prefers_dom_click(unknown));
        assert!(rect(-300.0, 10.0, 100.0, 40.0).prefers_dom_click(unknown));
    }

    #[test]
    fn quad_points_become_a_bounding_rect() {
        let rect =
            Rect::from_quad_points(&[-1.0, -1.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0]).expect("quad");
        assert_eq!(rect.x, -1.0);
        assert_eq!(rect.y, -1.0);
        assert_eq!(rect.width, 1.0);
        assert_eq!(rect.height, 1.0);
        assert!(Rect::from_quad_points(&[0.0, 0.0]).is_none());
    }
}
