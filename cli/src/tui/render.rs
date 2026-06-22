//! Core rendering traits — inspired by Codex's `Renderable` + `FlexRenderable`.
//!
//! Every widget implements [`Renderable`]. The layout engine is
//! [`FlexRenderable`] which distributes vertical space by weight
//! (flex-grow) — no manual `Constraint::Length` arithmetic.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

// ─── Renderable ───────────────────────────────────────────────────────────────

/// The fundamental rendering unit. Every widget, cell, and layout node
/// implements this.
pub trait Renderable {
    /// Render into the given area.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Height in terminal rows at the given width.
    fn desired_height(&self, width: u16) -> u16;

    /// Optional cursor position within the widget (for text input).
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

// ─── FlexRenderable ───────────────────────────────────────────────────────────

/// A vertical layout container. Children are distributed by flex weight:
/// - weight 0 = fixed (uses `desired_height`)
/// - weight > 0 = flex, shares remaining space proportionally
///
/// This replaces the error-prone `Layout::constraints` manual arithmetic.
pub struct FlexRenderable {
    children: Vec<FlexChild>,
}

struct FlexChild {
    weight: u16, // 0 = fixed, >0 = flex proportion
    renderable: Box<dyn Renderable>,
}

impl FlexRenderable {
    pub fn new() -> Self {
        Self { children: Vec::new() }
    }

    /// Add a fixed-size child (weight = 0).
    pub fn push_fixed(&mut self, child: Box<dyn Renderable>) {
        self.children.push(FlexChild { weight: 0, renderable: child });
    }

    /// Add a flex child that shares remaining space.
    pub fn push_flex(&mut self, weight: u16, child: Box<dyn Renderable>) {
        self.children.push(FlexChild { weight, renderable: child });
    }

    /// Compute the layout and return y-offsets for each child.
    fn layout(&self, area: Rect) -> Vec<Rect> {
        let total_height = area.height;
        let mut fixed_used = 0u16;
        let mut flex_total_weight = 0u16;

        for child in &self.children {
            if child.weight == 0 {
                fixed_used = fixed_used.saturating_add(child.renderable.desired_height(area.width));
            } else {
                flex_total_weight += child.weight;
            }
        }

        let remaining = total_height.saturating_sub(fixed_used);
        let mut y = area.y;
        let mut rects = Vec::with_capacity(self.children.len());

        for child in &self.children {
            let h = if child.weight == 0 {
                child.renderable.desired_height(area.width)
            } else if flex_total_weight > 0 {
                // Distribute proportionally
                (remaining as u32 * child.weight as u32 / flex_total_weight as u32) as u16
            } else {
                0
            };
            let rect = Rect { x: area.x, y, width: area.width, height: h };
            y = y.saturating_add(h);
            rects.push(rect);
        }

        rects
    }
}

impl Renderable for FlexRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let rects = self.layout(area);
        for (child, rect) in self.children.iter().zip(rects.iter()) {
            if rect.height > 0 {
                child.renderable.render(*rect, buf);
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter().map(|c| c.renderable.desired_height(width)).sum()
    }
}

impl Default for FlexRenderable {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Paragraph adapter ────────────────────────────────────────────────────────

/// Adapter that makes a ratatui `Paragraph` implement `Renderable`.
pub struct RenderableParagraph {
    pub lines: ratatui::text::Text<'static>,
    pub scroll: u16,
    pub trim: bool,
}

impl Renderable for RenderableParagraph {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let wrap = if self.trim {
            ratatui::widgets::Wrap { trim: true }
        } else {
            ratatui::widgets::Wrap { trim: false }
        };
        ratatui::widgets::Paragraph::new(self.lines.clone())
            .scroll((self.scroll, 0))
            .wrap(wrap)
            .render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let wrap = if self.trim {
            ratatui::widgets::Wrap { trim: true }
        } else {
            ratatui::widgets::Wrap { trim: false }
        };
        let count =
            ratatui::widgets::Paragraph::new(self.lines.clone()).wrap(wrap).line_count(width);
        count.try_into().unwrap_or(u16::MAX)
    }
}

// ─── Inset wrapper ─────────────────────────────────────────────────────────────

/// Wraps a `Renderable` with padding/insets.
pub struct InsetRenderable {
    pub inner: Box<dyn Renderable>,
    pub top: u16,
    pub bottom: u16,
    pub left: u16,
    pub right: u16,
}

impl Renderable for InsetRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner_area = Rect {
            x: area.x.saturating_add(self.left),
            y: area.y.saturating_add(self.top),
            width: area.width.saturating_sub(self.left).saturating_sub(self.right),
            height: area.height.saturating_sub(self.top).saturating_sub(self.bottom),
        };
        if inner_area.width > 0 && inner_area.height > 0 {
            self.inner.render(inner_area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        let inner_w = width.saturating_sub(self.left).saturating_sub(self.right);
        self.inner.desired_height(inner_w).saturating_add(self.top).saturating_add(self.bottom)
    }
}
