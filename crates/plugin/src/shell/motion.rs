//! The geometry of the chain ↔ block modal transition (`docs/research/mockups/block-modal.html`).
//!
//! The camera flies into the clicked block: the chain scales around the block's centre while
//! that centre travels to the modal's centre, and the block itself grows into the modal (the
//! box). The zoom factor is the size ratio of block and modal (the geometric mean of the width and
//! height ratios, so their areas match; the box absorbs the difference in aspect), and the scale
//! runs geometrically (`z^e`), so the zoom speed feels even although it ends around 8×.
//!
//! Walking the chain with the modal open (‹ ›, ← →) pans the camera from one block to the next
//! under the modal, which stays put.
//!
//! All rectangles are in the patch window's coordinates.

use std::time::{Duration, Instant};

use nice_plug_iced::iced::{Point, Rectangle, Size, Vector};

/// Containers are about as big as the modal already: they still get a camera move.
pub const MIN_ZOOM: f32 = 1.4;

/// How much the camera has to zoom for `node` to become `modal`.
pub fn zoom_factor(node: Size, modal: Size) -> f32 {
    let ratio = (modal.width / node.width.max(1.0)) * (modal.height / node.height.max(1.0));
    ratio.sqrt().max(MIN_ZOOM)
}

/// Ease in and out (cubic).
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// Scale `scale` around `origin`, then move `origin` by `offset` (what `zoom::Zoom` draws).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale: f32,
    pub origin: Point,
    pub offset: Vector,
}

impl Camera {
    /// Where a point of the chain lands on screen.
    pub fn map(&self, p: Point) -> Point {
        Point::new(
            self.origin.x + self.offset.x + (p.x - self.origin.x) * self.scale,
            self.origin.y + self.offset.y + (p.y - self.origin.y) * self.scale,
        )
    }
}

/// One frame of the opening (closing runs it backwards).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub camera: Camera,
    /// The box: the block, grown.
    pub rect: Rectangle,
    /// The modal's content, scaled to fit the box, and its top-left.
    pub body_scale: f32,
    pub body_at: Point,
}

/// The opening at eased progress `e` (0 = the block in the chain, 1 = the modal at `target`).
pub fn open(node: Rectangle, target: Rectangle, e: f32) -> Frame {
    let z = zoom_factor(node.size(), target.size());
    let s = z.powf(e);
    let (o, m) = (node.center(), target.center());
    let c = Point::new(lerp(o.x, m.x, e), lerp(o.y, m.y, e));
    // The block under the camera (its size × s), corrected towards the modal's aspect.
    let w = node.width * s * lerp(1.0, target.width / (node.width * z), e);
    let h = node.height * s * lerp(1.0, target.height / (node.height * z), e);
    let rect = Rectangle::new(Point::new(c.x - w / 2.0, c.y - h / 2.0), Size::new(w, h));
    let k = (w / target.width).min(h / target.height);
    let body_at = Point::new(rect.x + (w - target.width * k) / 2.0, rect.y + (h - target.height * k) / 2.0);
    Frame { camera: Camera { scale: s, origin: o, offset: (m - o) * e }, rect, body_scale: k, body_at }
}

/// The camera while walking from block `a` to block `b` at eased progress `e`: the point under the
/// modal's centre moves from one block's centre to the other's, the zoom from one factor to the
/// other.
pub fn pan(a: Rectangle, b: Rectangle, target: Rectangle, e: f32) -> Camera {
    let (za, zb) = (zoom_factor(a.size(), target.size()), zoom_factor(b.size(), target.size()));
    let scale = za * (zb / za).powf(e);
    let (pa, pb, m) = (a.center(), b.center(), target.center());
    let origin = Point::new(lerp(pa.x, pb.x, e), lerp(pa.y, pb.y, e));
    Camera { scale, origin, offset: m - origin }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// What runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Open,
    Close,
    /// From this block to the open one; `forward` = towards OUT.
    Pan { from: String, forward: bool },
}

/// A running transition.
#[derive(Debug, Clone)]
pub struct Motion {
    pub kind: Kind,
    pub start: Instant,
}

impl Motion {
    pub fn new(kind: Kind, now: Instant) -> Self {
        Self { kind, start: now }
    }

    /// A close that starts where an opening is now (`open` = its progress): runs back from there.
    pub fn close_from(open: f32, now: Instant) -> Self {
        let back = Self::duration().mul_f32((1.0 - open).clamp(0.0, 1.0));
        Self { kind: Kind::Close, start: now.checked_sub(back).unwrap_or(now) }
    }

    /// 280 ms (dev aid: `SCRIPTFX_DEV_ZOOM_MS=2000` slows it down to look at it); a pan a bit longer.
    pub fn duration() -> Duration {
        static D: std::sync::LazyLock<Duration> = std::sync::LazyLock::new(|| {
            let ms = std::env::var("SCRIPTFX_DEV_ZOOM_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(280);
            Duration::from_millis(ms)
        });
        *D
    }

    fn length(&self) -> Duration {
        match self.kind {
            Kind::Pan { .. } => Self::duration().mul_f32(1.15),
            _ => Self::duration(),
        }
    }

    /// 0..1, linear in time.
    pub fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.start).as_secs_f32() / self.length().as_secs_f32()).clamp(0.0, 1.0)
    }

    /// How open the modal is (linear): opening 0 → 1, closing 1 → 0, a pan stays open.
    pub fn openness(&self, now: Instant) -> f32 {
        match self.kind {
            Kind::Open => self.progress(now),
            Kind::Close => 1.0 - self.progress(now),
            Kind::Pan { .. } => 1.0,
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    const NODE: Rectangle = Rectangle { x: 300.0, y: 200.0, width: 80.0, height: 30.0 };
    const TARGET: Rectangle = Rectangle { x: 95.0, y: 140.0, width: 626.0, height: 266.0 };

    #[test]
    fn zoom_is_the_size_ratio_with_a_floor() {
        let z = zoom_factor(NODE.size(), TARGET.size());
        assert!(close(z, ((626.0 / 80.0) * (266.0 / 30.0) as f32).sqrt()));
        // The block, zoomed, has the modal's area.
        assert!(close(NODE.width * z * NODE.height * z, TARGET.width * TARGET.height));
        // A container already as big as the modal still zooms a little.
        assert_eq!(zoom_factor(Size::new(700.0, 300.0), TARGET.size()), MIN_ZOOM);
    }

    #[test]
    fn opening_starts_at_the_block_and_ends_at_the_modal() {
        let f = open(NODE, TARGET, 0.0);
        assert_eq!(f.rect, NODE);
        assert!(close(f.camera.scale, 1.0));
        assert_eq!(f.camera.map(Point::new(10.0, 20.0)), Point::new(10.0, 20.0));

        let f = open(NODE, TARGET, 1.0);
        assert!(close(f.rect.x, TARGET.x) && close(f.rect.y, TARGET.y));
        assert!(close(f.rect.width, TARGET.width) && close(f.rect.height, TARGET.height));
        assert!(close(f.body_scale, 1.0));
        // The block sits right behind the modal, zoomed by the size ratio.
        let c = f.camera.map(NODE.center());
        assert!(close(c.x, TARGET.center().x) && close(c.y, TARGET.center().y));
        assert!(close(f.camera.scale, zoom_factor(NODE.size(), TARGET.size())));
    }

    #[test]
    fn the_box_is_the_block_under_the_camera() {
        // Mid-way, the box is centred where the camera has the block, and the body fits inside it.
        let f = open(NODE, TARGET, 0.5);
        let c = f.camera.map(NODE.center());
        assert!(close(c.x, f.rect.center().x) && close(c.y, f.rect.center().y));
        assert!(f.body_scale > 0.0 && f.body_scale < 1.0);
        assert!(f.body_at.x >= f.rect.x - 1e-3 && f.body_at.x + TARGET.width * f.body_scale <= f.rect.x + f.rect.width + 1e-3);
    }

    #[test]
    fn a_pan_starts_and_ends_where_an_opening_ends() {
        let other = Rectangle { x: 420.0, y: 200.0, width: 96.0, height: 30.0 };
        let start = pan(NODE, other, TARGET, 0.0);
        let end = pan(NODE, other, TARGET, 1.0);
        let opened = open(NODE, TARGET, 1.0).camera;
        let p = Point::new(123.0, 45.0);
        assert!(close(start.map(p).x, opened.map(p).x) && close(start.map(p).y, opened.map(p).y));
        let c = end.map(other.center());
        assert!(close(c.x, TARGET.center().x) && close(c.y, TARGET.center().y));
    }

    #[test]
    fn a_close_runs_back_from_where_the_opening_is() {
        let now = Instant::now();
        let m = Motion::close_from(0.25, now);
        assert!(close(m.openness(now), 0.25));
        assert!(close(ease(0.0), 0.0) && close(ease(1.0), 1.0) && close(ease(0.5), 0.5));
    }
}
