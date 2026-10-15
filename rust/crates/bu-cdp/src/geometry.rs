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

    /// Tiny or off-viewport boxes cannot be hit with a compositor mouse event.
    ///
    /// Flutter Web's `flt-semantics-placeholder` is 1×1 CSS px at `(-1, -1)`.
    /// A mouse event at that centre misses the node; `HTMLElement.click()` does
    /// not.
    pub(crate) fn prefers_dom_click(self) -> bool {
        const MIN_MOUSE_EDGE: f64 = 2.0;
        (self.width < MIN_MOUSE_EDGE && self.height < MIN_MOUSE_EDGE)
            || self.x < 0.0
            || self.y < 0.0
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

    #[test]
    fn flutter_a11y_placeholder_prefers_dom_click() {
        let placeholder = Rect {
            x: -1.0,
            y: -1.0,
            width: 1.0,
            height: 1.0,
        };
        assert!(placeholder.prefers_dom_click());
        let (cx, cy) = placeholder.center();
        assert!((cx + 0.5).abs() < f64::EPSILON);
        assert!((cy + 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn in_viewport_form_controls_keep_mouse_click() {
        let email_overlay = Rect {
            x: 46.76,
            y: 288.06,
            width: 296.48,
            height: 20.0,
        };
        assert!(!email_overlay.prefers_dom_click());
        let cta = Rect {
            x: 16.0,
            y: 790.0,
            width: 358.0,
            height: 44.0,
        };
        assert!(!cta.prefers_dom_click());
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
