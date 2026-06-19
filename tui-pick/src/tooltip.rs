//! A floating-element positioning engine modeled on floating-ui / popper.js, adapted to a cell grid.
//!
//! Given an **anchor** rect, the floating element's **size**, the **viewport**, and a requested
//! [`Placement`] (side + cross-axis alignment), [`position`] computes the final on-screen rect after
//! applying two middleware, in order:
//!
//! - **flip** — if the element overflows the viewport on its placement side, try the opposite side;
//!   keep whichever loses less (or fits). This is why a `Bottom` tooltip near the screen bottom
//!   automatically renders above its anchor.
//! - **shift** — slide the element along its cross axis so it stays inside the viewport, without
//!   changing the side.
//!
//! The result reports the **resolved** placement (post-flip) so callers can, e.g., draw a pointer on
//! the correct edge. Everything is integer cell math and host-agnostic.

use ratatui::layout::Rect;

/// Which side of the anchor the floating element sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    /// The opposite side (used by the flip middleware).
    pub fn opposite(self) -> Self {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Whether the side runs along the vertical axis (top/bottom).
    pub fn is_vertical(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// Alignment of the floating element along the anchor's cross axis.
///
/// For a vertical side (top/bottom) the cross axis is horizontal: `Start` = left edges aligned.
/// For a horizontal side (left/right) the cross axis is vertical: `Start` = top edges aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// A placement = a [`Side`] plus a cross-axis [`Align`]. `bottom-start` (the column-header tooltip)
/// means "below the anchor, left edges aligned".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub side: Side,
    pub align: Align,
}

impl Placement {
    pub const fn new(side: Side, align: Align) -> Self {
        Placement { side, align }
    }

    /// `bottom-start`: below the anchor with left edges aligned (flips above when short on space).
    pub const fn bottom_start() -> Self {
        Placement { side: Side::Bottom, align: Align::Start }
    }

    /// `top-center`: above the anchor, centered.
    pub const fn top_center() -> Self {
        Placement { side: Side::Top, align: Align::Center }
    }
}

/// Options for [`position`].
#[derive(Debug, Clone, Copy)]
pub struct PositionOptions {
    /// Gap in cells between the anchor and the floating element along the main axis.
    pub offset: u16,
    /// Enable the flip middleware (swap to the opposite side when it doesn't fit).
    pub flip: bool,
    /// Enable the shift middleware (slide along the cross axis to stay on-screen).
    pub shift: bool,
}

impl Default for PositionOptions {
    fn default() -> Self {
        PositionOptions { offset: 0, flip: true, shift: true }
    }
}

/// The computed placement of a floating element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Positioned {
    /// Where to draw the floating element (already clamped to fit the viewport size).
    pub rect: Rect,
    /// The side actually used after flipping (may differ from the request).
    pub placement: Placement,
    /// Whether flip changed the side from the requested one.
    pub flipped: bool,
}

/// Space available between the anchor and the viewport edge on `side` (the main-axis room a tooltip
/// on that side could occupy, excluding the offset gap).
fn space_on(anchor: Rect, viewport: Rect, side: Side) -> u16 {
    match side {
        Side::Top => anchor.y.saturating_sub(viewport.y),
        Side::Bottom => viewport.bottom().saturating_sub(anchor.bottom()),
        Side::Left => anchor.x.saturating_sub(viewport.x),
        Side::Right => viewport.right().saturating_sub(anchor.right()),
    }
}

/// The main-axis origin (x for left/right, y for top/bottom) placing a `size`-long element on `side`.
fn main_axis_origin(anchor: Rect, side: Side, size: u16, offset: u16) -> i32 {
    match side {
        Side::Top => anchor.y as i32 - offset as i32 - size as i32,
        Side::Bottom => anchor.bottom() as i32 + offset as i32,
        Side::Left => anchor.x as i32 - offset as i32 - size as i32,
        Side::Right => anchor.right() as i32 + offset as i32,
    }
}

/// The cross-axis origin aligning a `size`-long element to the anchor per `align`.
fn cross_axis_origin(anchor_start: u16, anchor_len: u16, size: u16, align: Align) -> i32 {
    let start = anchor_start as i32;
    match align {
        Align::Start => start,
        Align::Center => start + (anchor_len as i32 - size as i32) / 2,
        Align::End => start + anchor_len as i32 - size as i32,
    }
}

/// Clamp `origin` so a `size`-long span stays within `[min, min+extent)`; if it can't fit, pin to min.
fn clamp_axis(origin: i32, size: u16, min: u16, extent: u16) -> u16 {
    let max = (min as i32 + extent as i32 - size as i32).max(min as i32);
    origin.clamp(min as i32, max) as u16
}

/// Position a floating element of `size` `(width, height)` relative to `anchor`, inside `viewport`,
/// applying flip then shift per `opts`. The returned rect is clamped to the viewport on both axes.
pub fn position(
    anchor: Rect,
    size: (u16, u16),
    viewport: Rect,
    requested: Placement,
    opts: PositionOptions,
) -> Positioned {
    let (width, height) = size;
    // The viewport may be smaller than the element; never produce a rect larger than the viewport.
    let width = width.min(viewport.width.max(1));
    let height = height.min(viewport.height.max(1));

    // --- flip middleware: pick the main-axis side ---
    let main_size = if requested.side.is_vertical() { height } else { width };
    let mut side = requested.side;
    let mut flipped = false;
    if opts.flip {
        let needed = main_size + opts.offset;
        if space_on(anchor, viewport, side) < needed {
            let other = side.opposite();
            // Flip only if the opposite side has strictly more room (else keep the request).
            if space_on(anchor, viewport, other) > space_on(anchor, viewport, side) {
                side = other;
                flipped = true;
            }
        }
    }

    // --- main + cross axis origins ---
    let main_origin = main_axis_origin(anchor, side, main_size, opts.offset);
    let (cross_origin, cross_size, cross_min, cross_extent) = if side.is_vertical() {
        (cross_axis_origin(anchor.x, anchor.width, width, requested.align), width, viewport.x, viewport.width)
    } else {
        (cross_axis_origin(anchor.y, anchor.height, height, requested.align), height, viewport.y, viewport.height)
    };

    // --- shift middleware: clamp the cross axis (and always clamp the main axis to the viewport) ---
    let cross = if opts.shift {
        clamp_axis(cross_origin, cross_size, cross_min, cross_extent)
    } else {
        cross_origin.max(cross_min as i32) as u16
    };
    let (main_min, main_extent) =
        if side.is_vertical() { (viewport.y, viewport.height) } else { (viewport.x, viewport.width) };
    let main = clamp_axis(main_origin, main_size, main_min, main_extent);

    let rect = if side.is_vertical() {
        Rect { x: cross, y: main, width, height }
    } else {
        Rect { x: main, y: cross, width, height }
    };
    Positioned { rect, placement: Placement { side, align: requested.align }, flipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Rect {
        Rect { x: 0, y: 0, width: 80, height: 24 }
    }

    #[test]
    fn bottom_start_places_below_left_aligned() {
        let anchor = Rect { x: 10, y: 5, width: 6, height: 1 };
        let pos = position(anchor, (20, 3), vp(), Placement::bottom_start(), PositionOptions::default());
        assert_eq!(pos.placement.side, Side::Bottom);
        assert!(!pos.flipped);
        assert_eq!(pos.rect.y, anchor.bottom()); // directly below, offset 0
        assert_eq!(pos.rect.x, anchor.x); // start-aligned
    }

    #[test]
    fn flips_above_when_no_room_below() {
        // Anchor at the very bottom: a tall tooltip can't fit below, must flip above.
        let anchor = Rect { x: 10, y: 22, width: 6, height: 1 };
        let pos = position(anchor, (20, 5), vp(), Placement::bottom_start(), PositionOptions::default());
        assert_eq!(pos.placement.side, Side::Top);
        assert!(pos.flipped);
        assert_eq!(pos.rect.bottom(), anchor.y); // sits just above the anchor
    }

    #[test]
    fn shift_keeps_wide_tooltip_on_screen_on_the_right_edge() {
        // Anchor near the right edge; a wide start-aligned tooltip would overflow → shift left.
        let anchor = Rect { x: 74, y: 5, width: 4, height: 1 };
        let pos = position(anchor, (30, 3), vp(), Placement::bottom_start(), PositionOptions::default());
        assert!(pos.rect.right() <= vp().right());
        assert_eq!(pos.rect.x, vp().right() - 30);
    }

    #[test]
    fn no_flip_when_disabled_even_without_room() {
        let anchor = Rect { x: 10, y: 22, width: 6, height: 1 };
        let opts = PositionOptions { offset: 0, flip: false, shift: true };
        let pos = position(anchor, (20, 5), vp(), Placement::bottom_start(), opts);
        assert_eq!(pos.placement.side, Side::Bottom);
        assert!(!pos.flipped);
    }

    #[test]
    fn offset_adds_a_gap() {
        let anchor = Rect { x: 10, y: 5, width: 6, height: 1 };
        let opts = PositionOptions { offset: 1, flip: true, shift: true };
        let pos = position(anchor, (10, 2), vp(), Placement::bottom_start(), opts);
        assert_eq!(pos.rect.y, anchor.bottom() + 1);
    }
}
