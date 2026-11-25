//! The **chain surface**: the audio tree, and the block modal that zooms out of it.
//!
//! This surface owns its own state — which block's modal is open and the transition running — so
//! the shell does not have to know what a block is (`docs/GLOSSARY.md`). The shell gives it a
//! [`Ctx`] each frame (the patch, the values, the pointer drag, the clock, how far the drawer is
//! out) and gets back a [`Look`]: where the modal sits this frame and how fast the camera moves.
//! Turning that into the renderer's blur stays with the shell, which is the only place that knows
//! what else is on screen and must stay sharp.
//!
//! See `motion` for the choreography and `docs/research/mockups/block-modal.html` for the design.

use std::time::Instant;

use nice_plug_iced::iced::{
    self, Border, Element, Fill, Shadow, Theme,
    widget::{column, container, mouse_area, pin, stack},
};
use patch::{BlockSpec, Patch, Values};

use super::{block_modal, chain_view, icons, knob, motion, notch, routing, stage, theme, zoom};

/// What the chain surface reports to the shell.
#[derive(Debug, Clone)]
pub enum Message {
    /// A block in the chain was clicked: open its modal.
    Open(String),
    /// ✕, a click beside the modal, or Escape.
    Close,
    /// ‹ › or ← →: walk to the previous / next block.
    Walk(bool),
    /// A knob in the open modal (param index). The shell applies it: param writes need the host.
    Knob(usize, knob::KnobEvent),
}

/// What the surface needs from the shell to lay itself out.
pub struct Ctx<'a> {
    pub patch: &'a Patch,
    pub values: &'a Values,
    /// A macro or modulator being dragged onto a knob.
    pub source_drag: Option<&'a routing::SourceDrag>,
    pub now: Instant,
    /// How far the drawer is out: the modal centres in the room it leaves.
    pub drawer_height: f32,
}

/// The chain and its block modal.
#[derive(Default)]
pub struct ChainSurface {
    /// The open block modal (block id).
    open: Option<String>,
    /// The chain ↔ modal transition (or a walk along the chain) while it runs.
    motion: Option<motion::Motion>,
}

impl ChainSurface {
    /// The block whose modal is open, if any.
    pub fn open_block(&self) -> Option<&str> {
        self.open.as_deref()
    }

    /// Click on a block: zoom in. Ignored while a modal is already open.
    pub fn open_modal(&mut self, id: String, now: Instant) {
        if self.open.is_some() {
            return;
        }
        self.open = Some(id);
        self.motion = Some(motion::Motion::new(motion::Kind::Open, now));
    }

    /// Back to the chain: from where an opening is now, or from the open modal.
    pub fn close_modal(&mut self, now: Instant) {
        if self.open.is_none() {
            return;
        }
        self.motion = match &self.motion {
            Some(m) if m.kind == motion::Kind::Close => return,
            Some(m) if m.kind == motion::Kind::Open => Some(motion::Motion::close_from(m.openness(now), now)),
            _ => Some(motion::Motion::new(motion::Kind::Close, now)),
        };
    }

    /// ‹ › and ← →: the previous / next block along the chain (into containers, depth-first).
    pub fn walk(&mut self, forward: bool, chain: &[BlockSpec], now: Instant) {
        let (Some(id), None) = (&self.open, &self.motion) else { return };
        let Some(next) = neighbour(chain, id, forward) else { return };
        self.motion = Some(motion::Motion::new(motion::Kind::Pan { from: id.clone(), forward }, now));
        self.open = Some(next);
    }

    /// Drop the modal at once (switching surfaces, unloading a patch).
    pub fn clear(&mut self) {
        self.open = None;
        self.motion = None;
    }

    /// Every frame: ends a finished transition and drops a modal whose block is gone (reload).
    /// `showing`: is this surface the one the viewport shows?
    pub fn tick(&mut self, patch: Option<&Patch>, showing: bool, now: Instant) {
        if let Some(m) = &self.motion
            && m.done(now)
        {
            if m.kind == motion::Kind::Close {
                self.open = None;
            }
            self.motion = None;
        }
        let gone = match (&self.open, patch) {
            (Some(id), Some(p)) => find_block(&p.chain, id).is_none(),
            (Some(_), None) => true,
            _ => false,
        };
        if gone || !showing {
            self.clear();
        }
    }

    /// Where the modal sits: centred in the room the notch and the drawer leave (it moves up as
    /// the drawer opens).
    fn target(&self, drawer_height: f32) -> iced::Rectangle {
        let (w, h) = theme::stage_size();
        let size = block_modal::SIZE;
        let top = notch::size().1 + theme::SPACE_2;
        let bottom = h - drawer_height - theme::SPACE_2;
        let y = ((top + bottom - size.height) / 2.0).max(top);
        iced::Rectangle::new(iced::Point::new((w - size.width) / 2.0, y), size)
    }

    /// Block `id` in the chain (as last drawn); a small box in the middle until it has been.
    fn node_rect(&self, id: &str, drawer_height: f32) -> iced::Rectangle {
        chain_view::rect(id).unwrap_or_else(|| {
            let c = self.target(drawer_height).center();
            iced::Rectangle::new(iced::Point::new(c.x - 40.0, c.y - 15.0), iced::Size::new(80.0, 30.0))
        })
    }

    /// The modal at this frame; `None` without one.
    pub fn look(&self, ctx: &Ctx<'_>) -> Option<Look> {
        let id = self.open.as_deref()?;
        let block = find_block(&ctx.patch.chain, id)?;
        let target = self.target(ctx.drawer_height);
        let node = self.node_rect(id, ctx.drawer_height);
        let container = chain_view::is_container(&block.kind);
        let still = Look {
            camera: motion::open(node, target, 1.0).camera,
            rect: target,
            radius: Look::RADIUS,
            box_alpha: 1.0,
            open: 1.0,
            body: Body { id: id.to_string(), scale: 1.0, at: target.position(), alpha: 1.0, title: true },
            label: None,
            velocity: 0.0,
            grid: motion::octaves(motion::zoom_factor(node.size(), target.size())),
        };
        let Some(m) = &self.motion else { return Some(still) };
        let p = m.progress(ctx.now);
        let velocity = (std::f32::consts::PI * p).sin();
        match &m.kind {
            motion::Kind::Open | motion::Kind::Close => {
                let e = motion::ease(m.openness(ctx.now));
                let f = motion::open(node, target, e);
                // The block's label grows into the title (riding along with the box).
                let small = chain_view::NAME / block_modal::TITLE_SIZE;
                let from = f.rect.position() + chain_view::label_offset(container, small * block_modal::TITLE_H);
                let to = f.body_at + iced::Vector::new(block_modal::TITLE_AT.x, block_modal::TITLE_AT.y) * f.body_scale;
                let label = (iced::Point::new(lerp(from.x, to.x, e), lerp(from.y, to.y, e)), lerp(small, f.body_scale, e));
                Some(Look {
                    camera: f.camera,
                    rect: f.rect,
                    radius: lerp(if container { 12.0 } else { theme::RADIUS_BUTTON }, Look::RADIUS, e),
                    // A container's card shows through until the box fills in.
                    box_alpha: if container { (e / 0.4).clamp(0.0, 1.0) } else { 1.0 },
                    open: e,
                    // The content fades in once the box is opaque (a container's box fills in first).
                    body: Body { id: id.to_string(), scale: f.body_scale, at: f.body_at, alpha: ((e - if container { 0.4 } else { 0.3 }) / 0.5).clamp(0.0, 1.0), title: container },
                    // A container's card shows its own header until the box covers it: no second
                    // label flying; its title comes in with the content.
                    label: (!container).then_some(label),
                    velocity,
                    grid: motion::grid_zoom(f.camera.scale, motion::zoom_factor(node.size(), target.size())),
                })
            }
            motion::Kind::Pan { from, forward } => {
                let e = motion::ease(p);
                let from_rect = self.node_rect(from, ctx.drawer_height);
                let camera = motion::pan(from_rect, node, target, e);
                let octaves = |r: iced::Rectangle| motion::octaves(motion::zoom_factor(r.size(), target.size()));
                let grid = lerp(octaves(from_rect), octaves(node), e);
                // The old content slides out, the new one in.
                let dir = if *forward { 1.0 } else { -1.0 };
                let (shown, alpha, dx) = if p < 0.5 { (from.clone(), 1.0 - 2.0 * p, -dir * 28.0 * 2.0 * p) } else { (id.to_string(), 2.0 * p - 1.0, dir * 28.0 * (2.0 - 2.0 * p)) };
                Some(Look {
                    camera,
                    body: Body { id: shown, scale: 1.0, at: target.position() + iced::Vector::new(dx, 0.0), alpha, title: true },
                    // The chain has faded out: only the dots slide, nothing to blur.
                    velocity: 0.0,
                    grid,
                    ..still
                })
            }
        }
    }

    /// The chain, with a block's modal over it: the camera on the chain, a veil, the box, the
    /// modal's content (scaled to the box, fading in), the block's label growing into the title,
    /// and the arrows beside the box.
    pub fn view<'a>(&'a self, ctx: &Ctx<'a>, look: Option<Look>) -> Element<'a, Message> {
        let (patch, values) = (ctx.patch, ctx.values);
        // The modal stands in for its block: the block isn't drawn under it (a container's card
        // only once the box covers it), nor the one a walk comes from.
        let open = self.open.as_deref();
        let is_card = open.and_then(|id| find_block(&patch.chain, id)).is_some_and(|b| chain_view::is_container(&b.kind));
        let hidden = match (&look, &self.motion) {
            (None, _) => [None, None],
            (Some(l), m) => {
                let from = match m.as_ref().map(|m| &m.kind) {
                    Some(motion::Kind::Pan { from, .. }) => Some(from.as_str()),
                    _ => None,
                };
                [open.filter(|_| !is_card || l.box_alpha >= 1.0), from]
            }
        };
        let chain = chain_view::view(patch, values, look.is_none(), hidden, look.is_none()).map(|chain_view::Event::Open(id)| Message::Open(id));
        let Some(look) = look else { return chain };
        let target = self.target(ctx.drawer_height);
        // The box's ground: the block's colour, solid (a container's card starts as the editor's
        // ground and turns into it).
        let ground = if is_card { mix(theme::c().frame_raised, theme::c().frame_node, (look.open / 0.5).clamp(0.0, 1.0)) } else { theme::c().frame_node };

        // The editor's ground, the chain under the camera fading out into it, and the ground's dot
        // grid zooming along (endlessly: it ends at its normal spacing). Open, the modal sits on the
        // plain editor.
        let open_t = look.open;
        let window = |colour: iced::Color| {
            move |_: &Theme| container::Style {
                background: Some(colour.into()),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
                ..Default::default()
            }
        };
        let ground_colour = theme::c().frame_raised;
        let world = zoom::zoom(chain, look.camera.scale, look.camera.origin).offset(look.camera.offset);
        let fade = ((open_t - 0.25) / 0.6).clamp(0.0, 1.0);
        let cam = look.camera;
        let d = 2f32.powf(look.grid);
        let first = iced::Point::new(chain_view::DOT_STEP / 2.0, chain_view::DOT_STEP / 2.0);
        let anchor = iced::Point::new(
            cam.origin.x + cam.offset.x + (first.x - cam.origin.x) * d,
            cam.origin.y + cam.offset.y + (first.y - cam.origin.y) * d,
        );
        // A click beside the modal closes it.
        let veil = mouse_area(container(chain_view::zoom_dots(anchor, look.grid)).width(Fill).height(Fill).style(window(theme::alpha(ground_colour, fade))))
            .on_press(Message::Close);
        let mut layers = stack![container(column![]).width(Fill).height(Fill).style(window(ground_colour)), world, veil, stage::corners()].width(Fill).height(Fill);

        // The box: the block, grown. `Idle`: clicks on it don't fall through to the veil.
        let (r, a, radius) = (look.rect, look.box_alpha, look.radius);
        let card = container(column![]).width(r.width).height(r.height).style(move |_: &Theme| container::Style {
            background: Some(theme::alpha(ground, a).into()),
            border: Border { radius: theme::radius(radius), width: 1.0, color: theme::alpha(theme::c().frame_hairline_strong, theme::c().frame_hairline_strong.a * a) },
            shadow: Shadow { color: theme::shadow(0.5 * open_t), offset: iced::Vector::new(0.0, 24.0 * open_t), blur_radius: 60.0 * open_t },
            ..Default::default()
        });
        layers = layers.push(pin(mouse_area(card).interaction(iced::mouse::Interaction::Idle)).x(r.x).y(r.y));

        // The content, laid out at the modal's place and drawn scaled into the box.
        if let Some(block) = find_block(&patch.chain, &look.body.id).filter(|_| look.body.alpha > 0.0) {
            let body = block_modal::view(patch, values, block, ctx.source_drag, look.body.title)
                .map(|m| match m {
                    block_modal::Message::Close => Message::Close,
                    block_modal::Message::Knob(i, e) => Message::Knob(i, e),
                });
            let offset = look.body.at - target.position();
            let body = container(pin(body).x(target.x).y(target.y)).width(Fill).height(Fill);
            layers = layers.push(zoom::zoom(body, look.body.scale, target.position()).offset(offset));
            // It fades in: a veil in the box's colour over it.
            if look.body.alpha < 1.0 {
                let veil = 1.0 - look.body.alpha;
                let cover = container(column![]).width(r.width).height(r.height).style(move |_: &Theme| container::Style {
                    background: Some(theme::alpha(ground, veil).into()),
                    border: Border { radius: theme::radius(radius), ..Default::default() },
                    ..Default::default()
                });
                layers = layers.push(pin(cover).x(r.x).y(r.y));
            }
        }
        // The block's label, growing into the title.
        if let (Some((at, scale)), Some(block)) = (look.label, open.and_then(|id| find_block(&patch.chain, id))) {
            let home = target.position() + iced::Vector::new(block_modal::TITLE_AT.x, block_modal::TITLE_AT.y);
            let title = container(pin(block_modal::title(patch, values, block, 1.0)).x(home.x).y(home.y)).width(Fill).height(Fill);
            layers = layers.push(zoom::zoom(title, scale, home).offset(at - home));
        }
        // ‹ › beside the box: floating chevrons, faint where the chain ends.
        let shown = ((look.open - 0.6) / 0.4).clamp(0.0, 1.0);
        if shown > 0.0 {
            let id = open.unwrap_or_default();
            let (left, right) = look.arrows();
            for (forward, at) in [(false, left), (true, right)] {
                let live = neighbour(&patch.chain, id, forward).is_some();
                let colour = theme::alpha(theme::c().frame_muted, shown * if live { 1.0 } else { 0.25 });
                let icon = icons::icon_rotated(icons::CHEVRON_RIGHT, 22.0, 2.0, colour, if forward { 0.0 } else { 180.0 });
                let cursor = if live { iced::mouse::Interaction::Pointer } else { iced::mouse::Interaction::Idle };
                let mut arrow = mouse_area(container(icon).center(Look::ARROW)).interaction(cursor);
                if live && look.open >= 1.0 && self.motion.is_none() {
                    arrow = arrow.on_press(Message::Walk(forward));
                }
                layers = layers.push(pin(arrow).x(at.x).y(at.y));
            }
        }
        layers.into()
    }
}

/// Where the modal is this frame, and how the chain behind it looks. The shell reads `rect`,
/// `velocity` and [`Look::arrows`] to build the renderer's blur.
pub struct Look {
    pub camera: motion::Camera,
    pub rect: iced::Rectangle,
    pub radius: f32,
    /// The box's opacity (a container's card shows through at first).
    pub box_alpha: f32,
    /// 0 = the chain, 1 = the modal open (eased): the chain fading out, the shadow.
    pub open: f32,
    pub body: Body,
    /// The block's label growing into the title: where, how big (`None` = the title draws itself).
    pub label: Option<(iced::Point, f32)>,
    /// How fast the camera moves (0..1): the zoom blur.
    pub velocity: f32,
    /// The ground's dot grid: how many octaves it has zoomed (see `chain_view::zoom_dots`).
    pub grid: f32,
}

/// The modal's content in a frame (a walk shows the previous block's while it slides out).
pub struct Body {
    pub id: String,
    pub scale: f32,
    pub at: iced::Point,
    pub alpha: f32,
    pub title: bool,
}

impl Look {
    /// The zoom blur's strength mid-way: how far a pixel smears towards the centre.
    pub const BLUR: f32 = 0.22;
    const RADIUS: f32 = 16.0;
    /// The arrows' hit area, and their gap to the box.
    const ARROW: f32 = 36.0;
    const ARROW_GAP: f32 = 14.0;

    /// Where ‹ and › sit: beside the box, centred on it.
    pub fn arrows(&self) -> (iced::Rectangle, iced::Rectangle) {
        let r = self.rect;
        let y = r.center_y() - Self::ARROW / 2.0;
        let size = iced::Size::new(Self::ARROW, Self::ARROW);
        (
            iced::Rectangle::new(iced::Point::new(r.x - Self::ARROW_GAP - Self::ARROW, y), size),
            iced::Rectangle::new(iced::Point::new(r.x + r.width + Self::ARROW_GAP, y), size),
        )
    }
}

fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color { r: lerp(a.r, b.r, t), g: lerp(a.g, b.g, t), b: lerp(a.b, b.b, t), a: lerp(a.a, b.a, t) }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The block before / after `id` along the chain (depth-first: a container, then its blocks).
pub fn neighbour(chain: &[BlockSpec], id: &str, forward: bool) -> Option<String> {
    let mut order = Vec::new();
    patch::chain::walk(chain, &mut |b| order.push(b.id.as_str()));
    let i = order.iter().position(|b| *b == id)?;
    let j = if forward { i + 1 } else { i.checked_sub(1)? };
    order.get(j).map(|s| s.to_string())
}

/// The block with this id, anywhere in the chain (containers included).
pub fn find_block<'a>(chain: &'a [BlockSpec], id: &str) -> Option<&'a BlockSpec> {
    let mut found = None;
    patch::chain::walk(chain, &mut |b| {
        if found.is_none() && b.id == id {
            found = Some(b);
        }
    });
    found
}
