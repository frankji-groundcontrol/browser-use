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

        if self.rects.len() + pending.len() > MAX_OCCLUSION_RECTS {
            return false;
        }

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
