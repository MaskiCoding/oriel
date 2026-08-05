use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, class, define_class,
    msg_send,
};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImageScaling, NSImageView, NSLineBreakMode,
    NSMutableParagraphStyle, NSPanel, NSParagraphStyleAttributeName, NSPopUpMenuWindowLevel,
    NSRunningApplication, NSScreen, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSArray, NSMutableAttributedString, NSNumber, NSObjectProtocol, NSPoint, NSPointInRect,
    NSRange, NSRect, NSSize, NSString,
};
use objc2_quartz_core::kCAGravityResizeAspect;
use objc2_quartz_core::{CABasicAnimation, CAGradientLayer, CAMediaTiming, CAMediaTimingFunction};

use crate::look::{Look, ShowOn, Size, Style, Theme, TitleShow, TitleTruncate};

/// Trackpad points (or equivalent) that must accumulate before one scroll step.
const SCROLL_STEP: f64 = 40.0;

/// One entry to render in the strip.
pub struct Tile {
    pub app: String,
    pub title: String,
    pub pid: i32,
    /// The real window's width over height; it shapes the tile.
    pub aspect: f64,
    /// A live window screenshot, when previews are available (Gallery style).
    /// `None` falls back to a large centered app icon.
    pub preview: Option<CFRetained<CGImage>>,
    /// State markers (minimized ●, fullscreen ⤢, Desktop number), already
    /// composed; empty shows no chip.
    pub badge: String,
    /// Byte ranges `(start, len)` in `title` to highlight, from the model's matcher.
    /// Empty = no highlight.
    pub title_spans: Vec<(usize, usize)>,
    /// Byte ranges `(start, len)` in `app` to highlight.
    pub app_spans: Vec<(usize, usize)>,
    /// An agent is working inside this window's app. Drawn as a slow drift of
    /// colour across the tile — the one thing in the strip that moves, and it
    /// moves because stillness is what says a window has finished.
    pub lantern: bool,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            app: String::new(),
            title: String::new(),
            pid: 0,
            aspect: 1.6,
            preview: None,
            badge: String::new(),
            title_spans: Vec::new(),
            app_spans: Vec::new(),
            lantern: false,
        }
    }
}

impl Tile {
    /// Builds a tile with no match highlighting.
    pub fn new(
        app: String,
        title: String,
        pid: i32,
        aspect: f64,
        preview: Option<CFRetained<CGImage>>,
        badge: String,
    ) -> Self {
        Self {
            app,
            title,
            pid,
            aspect,
            preview,
            badge,
            title_spans: Vec::new(),
            app_spans: Vec::new(),
            lantern: false,
        }
    }
}

/// Inner margin between a tile's edge and its contents.
const INSET: f64 = 8.0;
/// Height of the preview box inside every tile.
const PREVIEW_H: f64 = 140.0;
/// Height of the caption row (app icon + title) above the preview.
const CAPTION_H: f64 = 22.0;
const TILE_H: f64 = INSET + PREVIEW_H + 4.0 + CAPTION_H + 4.0;
/// Bounds on a tile's width, whatever the window's shape.
const MIN_W: f64 = 110.0;
const MAX_W: f64 = 360.0;
const GAP: f64 = 14.0;
const PAD: f64 = 24.0;
/// The fallback app icon size when a Gallery tile has no preview.
const ICON: f64 = 64.0;
/// Icons-style app icon at Medium — the icon is the tile's content.
const ICONS_ICON: f64 = 112.0;
/// Gap between the Icons-style icon and its title.
const ICONS_GAP: f64 = 6.0;
/// Separator when `[titles] show = "both"` — en dash with spaces.
const BOTH_SEP: &str = " – ";
/// Vertical padding inside a List row beyond icon/title.
const LIST_PAD_Y: f64 = 4.0;
/// Gap between List rows — menu-dense, not Gallery's tile gap.
const LIST_GAP: f64 = 2.0;
const LIST_MIN_W: f64 = 160.0;
const LIST_MAX_W: f64 = 360.0;
/// How full a row aims to be, as a share of the hard width limit — the strip
/// prefers growing down over stretching one row across the screen.
const ROW_FILL: f64 = 0.8;
/// Filter query row height at Medium scale (plus a gap before the tiles).
const QUERY_H: f64 = 26.0;
const QUERY_PROMPT: &str = "Filter";
/// Drawn caret width inside the query row.
const CARET_W: f64 = 1.5;
/// Optional dismiss fade; nothing ever animates in.
const FADE_OUT_SECS: f64 = 0.09;
const FADE_OUT_NS: i64 = 90_000_000;

/// Fixed scales for the discrete size settings; `Auto` picks in between.
const SCALE_SMALL: f64 = 0.7;
const SCALE_MEDIUM: f64 = 1.0;
const SCALE_LARGE: f64 = 1.4;
const SCALE_FLOOR: f64 = 0.2;
/// Stand-in window shape when the real aspects are not to hand.
const NOMINAL_ASPECT: f64 = 1.5;

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Tile metrics at a given scale. Scale 1.0 matches the Medium Gallery constants.
#[derive(Clone, Copy)]
struct Metrics {
    preview_h: f64,
    caption_h: f64,
    icon: f64,
    icons_icon: f64,
    min_w: f64,
    max_w: f64,
    gap: f64,
    list_gap: f64,
    list_pad_y: f64,
    /// Panel chrome for List — scales freely so a long column can still fit.
    list_pad: f64,
    pad: f64,
    inset: f64,
    title_font: f64,
    badge_font: f64,
    caption_icon: f64,
    list_min_w: f64,
    list_max_w: f64,
    query_h: f64,
    query_font: f64,
}

impl Metrics {
    fn at(scale: f64) -> Self {
        let s = scale;
        Self {
            preview_h: PREVIEW_H * s,
            // The caption draws with clamped font and icon sizes, so its band
            // must never shrink below what it actually renders.
            caption_h: (CAPTION_H * s).max(
                (16.0 * s)
                    .clamp(12.0, 20.0)
                    .max((12.0 * s).clamp(10.0, 14.0) + 5.0),
            ),
            icon: ICON * s,
            icons_icon: ICONS_ICON * s,
            min_w: MIN_W * s,
            max_w: MAX_W * s,
            gap: (GAP * s).max(6.0),
            // List densifies without the Gallery readability floors — Auto must
            // still keep a long column on one screen.
            list_gap: LIST_GAP * s,
            list_pad_y: LIST_PAD_Y * s,
            list_pad: PAD * s,
            pad: (PAD * s).max(10.0),
            inset: (INSET * s).max(3.0),
            title_font: (12.0 * s).clamp(10.0, 14.0),
            badge_font: (11.0 * s).clamp(9.0, 13.0),
            caption_icon: (16.0 * s).clamp(12.0, 20.0),
            // The readability floor must never overtake the ceiling: `clamp`
            // panics when min > max, and List densifies well past the point
            // where the scaled maximum drops under 80 pt.
            list_min_w: (LIST_MIN_W * s).max(80.0).min(LIST_MAX_W * s),
            list_max_w: LIST_MAX_W * s,
            query_h: (QUERY_H * s).clamp(20.0, 34.0),
            query_font: (13.0 * s).clamp(11.0, 16.0),
        }
    }

    fn gallery_tile_h(&self) -> f64 {
        self.inset + self.preview_h + 4.0 + self.caption_h + 4.0
    }

    fn icons_tile_w(&self) -> f64 {
        self.icons_icon + 2.0 * self.inset
    }

    fn icons_tile_h(&self) -> f64 {
        self.icons_icon + ICONS_GAP + self.caption_h + 2.0 * self.inset
    }

    /// Row height must follow the **clamped** caption metrics the row actually
    /// draws with. Recomputing them raw makes the row shorter than its own
    /// contents once List densifies past the clamp floors.
    fn list_row_h(&self) -> f64 {
        let line = self.title_font + 5.0;
        self.caption_icon.max(line) + 2.0 * self.list_pad_y
    }

    /// Extra panel height when the filter query row is visible (row + gap).
    fn query_space(&self) -> f64 {
        self.query_h + self.gap
    }
}

/// The chosen size is a **ceiling**, not a promise. The strip never scrolls
/// (PRD §4.6), so a fixed size that would not fit densifies down from there
/// exactly as `Auto` does — a large size with sixty windows still fits.
fn size_scale(
    size: Size,
    style: Style,
    tiles: usize,
    aspects: &[f64],
    screen_w: f64,
    screen_h: f64,
    query_space: f64,
) -> f64 {
    let ceiling = match size {
        Size::Small => SCALE_SMALL,
        Size::Medium => SCALE_MEDIUM,
        // Auto has no ceiling of its own beyond the largest step.
        Size::Large | Size::Auto => SCALE_LARGE,
    };
    scale_under(
        ceiling,
        style,
        tiles,
        aspects,
        screen_w,
        screen_h,
        query_space,
    )
}

/// Picks a scale so every tile fits on one screen for the active style. More
/// windows never yield a larger scale; the floor keeps a huge session readable
/// instead of collapsing to zero.
#[cfg(test)]
fn auto_scale(style: Style, tiles: usize, screen_w: f64, screen_h: f64) -> f64 {
    scale_under(SCALE_LARGE, style, tiles, &[], screen_w, screen_h, 0.0)
}

/// Largest candidate scale at or below `ceiling` whose panel fits the screen.
fn scale_under(
    ceiling: f64,
    style: Style,
    tiles: usize,
    aspects: &[f64],
    screen_w: f64,
    screen_h: f64,
    query_space: f64,
) -> f64 {
    if tiles == 0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return ceiling.min(SCALE_MEDIUM);
    }
    // List is a single column — densify below the shared floor when needed.
    let candidates: &[f64] = match style {
        Style::List => &[
            SCALE_LARGE,
            1.2,
            1.1,
            SCALE_MEDIUM,
            0.9,
            0.8,
            SCALE_SMALL,
            0.6,
            0.5,
            0.4,
            0.3,
            SCALE_FLOOR,
            0.15,
            0.1,
            0.075,
            0.05,
        ],
        Style::Gallery | Style::Icons => &[
            SCALE_LARGE,
            1.2,
            1.1,
            SCALE_MEDIUM,
            0.9,
            0.8,
            SCALE_SMALL,
            0.6,
            0.5,
            0.4,
            0.3,
            SCALE_FLOOR,
        ],
    };
    for &scale in candidates {
        if scale <= ceiling
            && panel_fits(
                style,
                tiles,
                aspects,
                screen_w,
                screen_h,
                scale,
                query_space,
            )
        {
            return scale;
        }
    }
    match style {
        Style::List => 0.05,
        Style::Gallery | Style::Icons => SCALE_FLOOR,
    }
}

fn gallery_tile_width(aspect: f64, m: &Metrics) -> f64 {
    (aspect * m.preview_h).clamp(m.min_w, m.max_w) + 2.0 * m.inset
}

/// Approximate system-font label width without `AppKit` — enough to size the
/// List column from its longest title.
fn text_width(text: &str, font_size: f64) -> f64 {
    as_f64(text.chars().count()) * font_size * 0.62
}

fn tile_label(tile: &Tile, show: TitleShow) -> String {
    caption_parts(tile, show).0
}

/// Caption string and match spans for the configured `show` mode.
/// Spans stay byte ranges into the returned string (not truncated).
fn caption_parts(tile: &Tile, show: TitleShow) -> (String, Vec<(usize, usize)>) {
    match show {
        TitleShow::App => (tile.app.clone(), tile.app_spans.clone()),
        TitleShow::Title => {
            if tile.title.is_empty() {
                (tile.app.clone(), tile.app_spans.clone())
            } else {
                (tile.title.clone(), tile.title_spans.clone())
            }
        }
        TitleShow::Both => {
            if tile.title.is_empty() {
                (tile.app.clone(), tile.app_spans.clone())
            } else if tile.app.is_empty() {
                (tile.title.clone(), tile.title_spans.clone())
            } else {
                let mut text =
                    String::with_capacity(tile.app.len() + BOTH_SEP.len() + tile.title.len());
                text.push_str(&tile.app);
                text.push_str(BOTH_SEP);
                text.push_str(&tile.title);
                let offset = tile.app.len() + BOTH_SEP.len();
                let mut spans = tile.app_spans.clone();
                spans.extend(
                    tile.title_spans
                        .iter()
                        .map(|&(start, len)| (start.saturating_add(offset), len)),
                );
                (text, spans)
            }
        }
    }
}

fn list_content_width(tiles: &[Tile], m: &Metrics, show: TitleShow, markers: bool) -> f64 {
    let mut max_title: f64 = 48.0;
    let mut max_badge: f64 = 0.0;
    for tile in tiles {
        max_title = max_title.max(text_width(&tile_label(tile, show), m.title_font));
        if markers && !tile.badge.is_empty() {
            max_badge = max_badge.max(text_width(&tile.badge, m.badge_font));
        }
    }
    let icon_slot = m.caption_icon + 6.0;
    let badge_slot = if max_badge > 0.0 {
        max_badge + 6.0
    } else {
        0.0
    };
    (2.0 * m.inset + icon_slot + max_title + badge_slot).clamp(m.list_min_w, m.list_max_w)
}

/// `aspects` are the real window shapes when the caller has them. A Gallery
/// tile's width follows its window's aspect, so probing with one nominal shape
/// under-measures a row of wide windows and lets the panel overflow.
fn panel_fits(
    style: Style,
    tiles: usize,
    aspects: &[f64],
    screen_w: f64,
    screen_h: f64,
    scale: f64,
    query_space: f64,
) -> bool {
    let m = Metrics::at(scale);
    let plan = match style {
        Style::Gallery => {
            let widths: Vec<f64> = if aspects.is_empty() {
                vec![gallery_tile_width(NOMINAL_ASPECT, &m); tiles]
            } else {
                aspects.iter().map(|&a| gallery_tile_width(a, &m)).collect()
            };
            let max_content = (screen_w * 0.9 - 2.0 * m.pad).max(m.min_w);
            layout_sized(
                &widths,
                max_content,
                m.gallery_tile_h(),
                m.gap,
                m.pad,
                query_space,
            )
        }
        Style::Icons => {
            let tw = m.icons_tile_w();
            let widths = vec![tw; tiles];
            let max_content = (screen_w * 0.9 - 2.0 * m.pad).max(tw);
            layout_sized(
                &widths,
                max_content,
                m.icons_tile_h(),
                m.gap,
                m.pad,
                query_space,
            )
        }
        Style::List => {
            // Worst-case column width — titles aren't known at auto-scale time.
            list_layout(
                tiles,
                m.list_max_w,
                m.list_row_h(),
                m.list_gap,
                m.list_pad,
                query_space,
                screen_h,
            )
        }
    };
    plan.width <= screen_w && plan.height <= screen_h
}

/// A tile's width follows its window's aspect at the shared preview height,
/// within limits that keep captions readable and panoramas polite.
#[cfg(test)]
fn tile_width(aspect: f64) -> f64 {
    (aspect * PREVIEW_H).clamp(MIN_W, MAX_W) + 2.0 * INSET
}

/// Where every tile goes: bottom-left origins in panel coordinates, rows
/// balanced to near-equal widths and centered, plus the panel size.
struct Layout {
    origins: Vec<(f64, f64)>,
    /// Tile indices per visual row, top row first.
    rows: Vec<Vec<usize>>,
    width: f64,
    height: f64,
}

/// Index of the tile whose frame contains `point`, if any.
fn hit_tile(frames: &[NSRect], point: NSPoint) -> Option<usize> {
    frames.iter().position(|frame| NSPointInRect(point, *frame))
}

/// Fold a scroll delta into an accumulator; emit ±1 per `SCROLL_STEP` crossing.
/// Positive `delta_y` (fingers up) yields −1; negative yields +1.
fn scroll_steps(accum: &mut f64, delta_y: f64, precise: bool) -> Vec<i32> {
    if delta_y == 0.0 {
        return Vec::new();
    }
    let delta = if precise {
        delta_y
    } else {
        delta_y.signum() * SCROLL_STEP
    };
    *accum += delta;
    let mut out = Vec::new();
    while *accum >= SCROLL_STEP {
        *accum -= SCROLL_STEP;
        out.push(-1);
    }
    while *accum <= -SCROLL_STEP {
        *accum += SCROLL_STEP;
        out.push(1);
    }
    out
}

#[cfg(test)]
fn layout(widths: &[f64], max_content: f64) -> Layout {
    layout_sized(widths, max_content, TILE_H, GAP, PAD, 0.0)
}

#[cfg(test)]
fn layout_with_query(widths: &[f64], max_content: f64, query_space: f64) -> Layout {
    layout_sized(widths, max_content, TILE_H, GAP, PAD, query_space)
}

fn layout_sized(
    widths: &[f64],
    max_content: f64,
    tile_h: f64,
    gap: f64,
    pad: f64,
    query_space: f64,
) -> Layout {
    if widths.is_empty() {
        return Layout {
            origins: Vec::new(),
            rows: Vec::new(),
            width: 2.0 * pad,
            height: 2.0 * pad + query_space,
        };
    }
    let total = widths.iter().sum::<f64>() + gap * as_f64(widths.len().saturating_sub(1));
    // Enough rows that none needs to fill past ROW_FILL of the hard limit...
    let mut wanted = 1;
    while total / as_f64(wanted) > max_content * ROW_FILL && wanted < widths.len() {
        wanted += 1;
    }
    // ...then hand tiles out so every row lands as close to its share as the
    // tile sizes allow. The last row takes the remainder; the hard limit
    // always wins.
    let target = total / as_f64(wanted);
    let mut rows: Vec<(Vec<usize>, f64)> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut row_w = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        let next = row_w + gap + w;
        let over_hard = next > max_content;
        let past_share = rows.len() + 1 < wanted && (next - target).abs() > (target - row_w).abs();
        if !row.is_empty() && (over_hard || past_share) {
            rows.push((std::mem::take(&mut row), row_w));
            row_w = 0.0;
        }
        row_w += if row.is_empty() { w } else { gap + w };
        row.push(i);
    }
    if !row.is_empty() {
        rows.push((row, row_w));
    }
    let content_w = rows.iter().map(|r| r.1).fold(0.0, f64::max);
    let count = as_f64(rows.len());
    let width = content_w + 2.0 * pad;
    let height = count.mul_add(tile_h, (count - 1.0).max(0.0) * gap) + 2.0 * pad + query_space;
    let mut origins = vec![(0.0, 0.0); widths.len()];
    let mut row_indices = Vec::with_capacity(rows.len());
    for (r, (indices, w)) in rows.iter().enumerate() {
        let mut x = pad + (content_w - w) / 2.0;
        let y = height - pad - query_space - as_f64(r + 1) * tile_h - as_f64(r) * gap;
        for &i in indices {
            origins[i] = (x, y);
            x += widths[i] + gap;
        }
        row_indices.push(indices.clone());
    }
    Layout {
        origins,
        rows: row_indices,
        width,
        height,
    }
}

/// Usable screen box the strip must fit inside.
#[derive(Clone, Copy)]
struct Screen {
    w: f64,
    h: f64,
}

fn plan(
    tiles: &[Tile],
    style: Style,
    m: &Metrics,
    screen: Screen,
    query_space: f64,
    show: TitleShow,
    markers: bool,
) -> Layout {
    match style {
        Style::Gallery => {
            let widths: Vec<f64> = tiles
                .iter()
                .map(|t| gallery_tile_width(t.aspect, m))
                .collect();
            let max_content = (screen.w * 0.9 - 2.0 * m.pad).max(m.min_w);
            layout_sized(
                &widths,
                max_content,
                m.gallery_tile_h(),
                m.gap,
                m.pad,
                query_space,
            )
        }
        Style::Icons => {
            let tw = m.icons_tile_w();
            let widths = vec![tw; tiles.len()];
            let max_content = (screen.w * 0.9 - 2.0 * m.pad).max(tw);
            layout_sized(
                &widths,
                max_content,
                m.icons_tile_h(),
                m.gap,
                m.pad,
                query_space,
            )
        }
        Style::List => list_layout(
            tiles.len(),
            list_content_width(tiles, m, show, markers),
            m.list_row_h(),
            m.list_gap,
            m.list_pad,
            query_space,
            screen.h,
        ),
    }
}

/// List rows down a column, wrapping into further columns when one column
/// would run past `max_h`. The strip never scrolls (PRD §4.6), and the caption
/// metrics have clamp floors, so past a certain count height alone cannot give.
fn list_layout(
    n: usize,
    row_w: f64,
    row_h: f64,
    gap: f64,
    pad: f64,
    query_space: f64,
    max_h: f64,
) -> Layout {
    if n == 0 {
        return Layout {
            origins: Vec::new(),
            rows: Vec::new(),
            width: row_w + 2.0 * pad,
            height: 2.0 * pad + query_space,
        };
    }
    let usable = (max_h - 2.0 * pad - query_space).max(row_h);
    // How many rows fit in one column: k*row_h + (k-1)*gap <= usable.
    // Rows that fit one column: k*row_h + (k-1)*gap <= usable. Counted by
    // stepping rather than casting a float, so no truncation or sign games.
    let mut per_col = 1;
    while per_col < n && as_f64(per_col + 1).mul_add(row_h, as_f64(per_col) * gap) <= usable {
        per_col += 1;
    }
    let cols = n.div_ceil(per_col);
    let rows_used = per_col.min(n);

    let count = as_f64(rows_used);
    let width = as_f64(cols).mul_add(row_w, as_f64(cols - 1) * gap) + 2.0 * pad;
    let height = count.mul_add(row_h, (count - 1.0) * gap) + 2.0 * pad + query_space;

    let mut origins = Vec::with_capacity(n);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); rows_used];
    for (i, slot) in (0..n).enumerate() {
        let col = slot / per_col;
        let row = slot % per_col;
        let x = pad + as_f64(col) * (row_w + gap);
        let y = height - pad - query_space - as_f64(row + 1) * row_h - as_f64(row) * gap;
        origins.push((x, y));
        rows[row].push(i);
    }
    Layout {
        origins,
        rows,
        width,
        height,
    }
}

/// The selection lifts the whole tile: a lighter backing plus an accent ring.
/// A working window is marked by the drifting pearl laid over it instead.
///
/// The two stay tellable apart when they land on the same tile: the accent ring
/// always belongs to the selection, the shimmer always belongs to the light.
/// How a tile should be drawn. Named rather than three bare bools: the call
/// sites read `Emphasis { selected, lit, dark }` instead of `(true, false,
/// true)`, which cannot be silently mis-ordered.
#[derive(Clone, Copy)]
struct Emphasis {
    selected: bool,
    lit: bool,
    dark: bool,
}

fn set_highlight(tile: &NSView, how: Emphasis) {
    let Emphasis {
        selected: on,
        lit,
        dark,
    } = how;
    let Some(layer) = tile.layer() else {
        return;
    };
    let border = if on {
        Some(NSColor::controlAccentColor().CGColor())
    } else if lit {
        // A pale shell edge, not the accent ring — the drift already says the
        // window is working, so the border only has to close the shape.
        Some(NSColor::colorWithSRGBRed_green_blue_alpha(0.96, 0.94, 0.98, 0.30).CGColor())
    } else {
        None
    };
    layer.setBorderColor(border.as_deref());
    layer.setBorderWidth(if on {
        2.5
    } else if lit {
        1.0
    } else {
        0.0
    });

    // Only the selection gets a surface. A preview is already a picture of a
    // window with its own edges, so a panel behind every tile reads as chrome
    // the window does not have.
    //
    // Nothing tints a lit tile either: the light is the drift laid over it, and
    // a coloured backing underneath only turned that back into a stain.
    // No card behind a tile. A preview is already a picture of a window with
    // its own edges; boxing it in a second panel reads as chrome the window
    // does not have. Only the selection gets a surface, and only enough of one
    // to sit under the caption.
    let backing = on.then(|| {
        if dark {
            NSColor::colorWithWhite_alpha(1.0, 0.14).CGColor()
        } else {
            NSColor::colorWithWhite_alpha(0.0, 0.10).CGColor()
        }
    });
    layer.setBackgroundColor(backing.as_deref());
}

/// An image view that scales its image to fill `side` in both directions.
/// `NSRunningApplication`'s icon reports a 32 pt size and the default scaling
/// mode never enlarges, so a large icon tile would otherwise draw it tiny.
fn scaled_icon_view(
    mtm: MainThreadMarker,
    icon: &objc2_app_kit::NSImage,
    frame: NSRect,
) -> Retained<NSImageView> {
    let view = NSImageView::new(mtm);
    view.setImage(Some(icon));
    view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.setFrame(frame);
    view
}

fn app_icon_uncached(pid: i32) -> Option<Retained<objc2_app_kit::NSImage>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).and_then(|app| app.icon())
}

fn mouse_location() -> NSPoint {
    // SAFETY: `+[NSEvent mouseLocation]` returns an `NSPoint` by value.
    unsafe { msg_send![class!(NSEvent), mouseLocation] }
}

/// `ORIEL_MATERIAL=none` takes the behind-window blur out of the picture
/// entirely — flat fill, no sampling of what is behind — so a rendering problem
/// can be tested against a panel that asks nothing of the compositor.
fn vibrancy_off() -> bool {
    std::env::var("ORIEL_MATERIAL").is_ok_and(|v| v == "none")
}

/// The vibrancy material. `ORIEL_MATERIAL` overrides it while a look is being
/// chosen; the default is what ships.
fn material_for(dark: bool) -> NSVisualEffectMaterial {
    match std::env::var("ORIEL_MATERIAL").unwrap_or_default().as_str() {
        "hud" => NSVisualEffectMaterial::HUDWindow,
        "menu" => NSVisualEffectMaterial::Menu,
        "popover" => NSVisualEffectMaterial::Popover,
        "sidebar" => NSVisualEffectMaterial::Sidebar,
        "under" => NSVisualEffectMaterial::UnderWindowBackground,
        "header" => NSVisualEffectMaterial::HeaderView,
        "sheet" => NSVisualEffectMaterial::Sheet,
        "window" => NSVisualEffectMaterial::WindowBackground,
        _ if dark => NSVisualEffectMaterial::Menu,
        _ => NSVisualEffectMaterial::Popover,
    }
}

/// One field of the lantern: a soft ellipse of colour that drifts across the
/// tile. Sized so its brighter middle lands on the preview — close enough to
/// have presence rather than being a flat wash — and turned to its own angle,
/// because a circle announces itself as light thrown onto the window from
/// somewhere else. Anchored away from the centre too: concentric fields look
/// emitted from the middle of the window rather than sitting on it.
struct Field {
    rgb: (f64, f64, f64),
    alpha: f64,
    size: f64,
    /// Width over height. Anything but 1.0 turns the circle into an ellipse.
    stretch: f64,
    tilt: f64,
    /// How far it travels, as a fraction of the tile's longest edge.
    sway: f64,
    /// Seconds for a full x and y traverse. Deliberately unequal and sharing no
    /// common factor, so the two never resynchronise and the path stays open
    /// instead of retracing a loop.
    periods: (f64, f64),
    anchor: (f64, f64),
}

/// Four is enough to keep the colour recombining without any one dominating.
const LANTERN_FIELDS: [Field; 4] = [
    Field {
        rgb: (0.30, 0.74, 1.00),
        alpha: 0.17,
        size: 1.35,
        stretch: 1.8,
        tilt: 0.40,
        sway: 0.20,
        periods: (13.0, 17.0),
        anchor: (-0.26, 0.18),
    },
    Field {
        rgb: (0.58, 0.42, 1.00),
        alpha: 0.15,
        size: 1.55,
        stretch: 1.5,
        tilt: -0.70,
        sway: 0.22,
        periods: (19.0, 11.0),
        anchor: (0.30, -0.14),
    },
    Field {
        rgb: (0.34, 1.00, 0.78),
        alpha: 0.13,
        size: 1.25,
        stretch: 2.1,
        tilt: 1.10,
        sway: 0.18,
        periods: (23.0, 15.0),
        anchor: (0.14, 0.28),
    },
    Field {
        rgb: (1.00, 0.48, 0.72),
        alpha: 0.12,
        size: 1.45,
        stretch: 1.6,
        tilt: -0.30,
        sway: 0.21,
        periods: (16.0, 21.0),
        anchor: (-0.18, -0.24),
    },
];

/// Long enough to read as ebbing rather than blinking, short enough that the
/// strip is not still settling by the time it is dismissed.
const LANTERN_FADE: f64 = 0.55;
const LANTERN_FADE_NS: i64 = 550_000_000;

/// Animates a view's opacity without waiting on a completion callback.
fn fade_view(view: &NSView, to: f64, seconds: f64) {
    let Some(layer) = view.layer() else {
        view.setAlphaValue(to);
        return;
    };
    let from = f64::from(layer.opacity());
    let anim = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
    // SAFETY: the `opacity` key path takes a plain number.
    unsafe {
        anim.setFromValue(Some(&NSNumber::new_f64(from)));
        anim.setToValue(Some(&NSNumber::new_f64(to)));
    }
    anim.setDuration(seconds);
    layer.addAnimation_forKey(&anim, Some(&NSString::from_str("fade")));
    #[allow(clippy::cast_possible_truncation)]
    layer.setOpacity(to as f32);
}

/// A stable number per window, so its lantern always drifts on its own phase.
/// Two windows of one app must not share it, so the title contributes too.
fn drift_seed(tile: &Tile) -> u64 {
    let mut seed = u64::from(tile.pid.unsigned_abs()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for byte in tile.title.bytes().chain(tile.app.bytes()) {
        seed = (seed ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3);
    }
    seed
}

/// `CoreAnimation` wants gradient stops as boxed numbers.
fn stops(values: &[f64]) -> Retained<NSArray<NSNumber>> {
    let boxed: Vec<Retained<NSNumber>> = values.iter().map(|v| NSNumber::new_f64(*v)).collect();
    NSArray::from_retained_slice(&boxed)
}

fn system_appearance_is_dark() -> bool {
    // SAFETY: AppKit singletons; `name` is an NSString.
    let app: Retained<AnyObject> = unsafe { msg_send![class!(NSApplication), sharedApplication] };
    let appearance: Retained<AnyObject> = unsafe { msg_send![&*app, effectiveAppearance] };
    let name: Retained<NSString> = unsafe { msg_send![&*appearance, name] };
    name.to_string().contains("Dark")
}

fn theme_is_dark(theme: Theme) -> bool {
    match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => system_appearance_is_dark(),
    }
}

/// Convert a UTF-8 byte span `(start, len)` into an `AppKit` UTF-16 `NSRange`.
/// Returns `None` for empty, out-of-bounds, or non-char-boundary spans.
fn utf8_span_to_utf16_range(text: &str, start: usize, len: usize) -> Option<NSRange> {
    let end = start.checked_add(len)?;
    if len == 0 || end > text.len() {
        return None;
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    let utf16_start = text[..start].encode_utf16().count();
    let utf16_len = text[start..end].encode_utf16().count();
    Some(NSRange::new(utf16_start, utf16_len))
}

fn attributed_label(
    text: &str,
    spans: &[(usize, usize)],
    font_size: f64,
    base_color: &NSColor,
    truncate: TitleTruncate,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let ns = NSString::from_str(text);
    let attr = NSMutableAttributedString::from_nsstring(&ns);
    let full = NSRange::new(0, ns.length());
    let font = NSFont::systemFontOfSize(font_size);
    let para = NSMutableParagraphStyle::new();
    para.setLineBreakMode(match truncate {
        TitleTruncate::Start => NSLineBreakMode::ByTruncatingHead,
        TitleTruncate::Middle => NSLineBreakMode::ByTruncatingMiddle,
        TitleTruncate::End => NSLineBreakMode::ByTruncatingTail,
    });
    // SAFETY: font/color/paragraph style are valid attribute values for the given keys.
    unsafe {
        attr.addAttribute_value_range(NSFontAttributeName, &font, full);
        attr.addAttribute_value_range(NSForegroundColorAttributeName, base_color, full);
        attr.addAttribute_value_range(NSParagraphStyleAttributeName, &para, full);
    }
    if !spans.is_empty() {
        let accent = NSColor::controlAccentColor();
        for &(start, len) in spans {
            let Some(range) = utf8_span_to_utf16_range(text, start, len) else {
                continue;
            };
            // SAFETY: accent is a valid foreground colour attribute value.
            unsafe {
                attr.addAttribute_value_range(NSForegroundColorAttributeName, &accent, range);
            }
        }
    }
    let label = NSTextField::labelWithAttributedString(&attr, mtm);
    label.setMaximumNumberOfLines(1);
    label
}

/// Cancels a pending fade-out `orderOut` when the generation no longer matches.
/// A lantern overlay waiting for its fade to finish so it can be removed.
struct LanternDone {
    glass: Retained<NSView>,
    generation: Rc<Cell<u32>>,
    stamp: u32,
}

unsafe extern "C" fn lantern_fade_finished(ctx: *mut c_void) {
    // SAFETY: paired with `Box::into_raw` in `Strip::set_lantern`.
    let done = unsafe { Box::from_raw(ctx.cast::<LanternDone>()) };
    // A re-summon rebuilds every tile; removing then would touch a view that no
    // longer belongs to anything.
    if done.generation.get() == done.stamp {
        done.glass.removeFromSuperview();
    }
}

struct FadeDone {
    panel: Retained<NSPanel>,
    generation: Rc<Cell<u32>>,
    stamp: u32,
}

unsafe extern "C" fn fade_out_finished(ctx: *mut c_void) {
    // SAFETY: paired with `Box::into_raw` in `Strip::hide`.
    let done = unsafe { Box::from_raw(ctx.cast::<FadeDone>()) };
    if done.generation.get() != done.stamp {
        return;
    }
    done.panel.orderOut(None);
    done.panel.setAlphaValue(1.0);
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_time(when: u64, delta: i64) -> u64;
    fn dispatch_after_f(
        when: u64,
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

/// Shared mouse / layout state reached from both `Strip` and its content view.
type ClickCb = Box<dyn FnMut(usize)>;
type ScrollCb = Box<dyn FnMut(i32)>;
type HoverCb = Box<dyn FnMut(usize)>;

struct MouseState {
    on_click: RefCell<Option<ClickCb>>,
    on_scroll: RefCell<Option<ScrollCb>>,
    on_hover: RefCell<Option<HoverCb>>,
    hover_select: Cell<bool>,
    scroll_accum: Cell<f64>,
    last_hover: Cell<Option<usize>>,
    frames: RefCell<Vec<NSRect>>,
}

impl MouseState {
    fn new() -> Self {
        Self {
            on_click: RefCell::new(None),
            on_scroll: RefCell::new(None),
            on_hover: RefCell::new(None),
            hover_select: Cell::new(false),
            scroll_accum: Cell::new(0.0),
            last_hover: Cell::new(None),
            frames: RefCell::new(Vec::new()),
        }
    }

    fn fire_click(&self, index: usize) {
        let mut slot = self.on_click.borrow_mut();
        let Some(mut f) = slot.take() else {
            return;
        };
        drop(slot);
        f(index);
        *self.on_click.borrow_mut() = Some(f);
    }

    fn fire_scroll(&self, delta: i32) {
        let mut slot = self.on_scroll.borrow_mut();
        let Some(mut f) = slot.take() else {
            return;
        };
        drop(slot);
        f(delta);
        *self.on_scroll.borrow_mut() = Some(f);
    }

    fn fire_hover(&self, index: usize) {
        let mut slot = self.on_hover.borrow_mut();
        let Some(mut f) = slot.take() else {
            return;
        };
        drop(slot);
        f(index);
        *self.on_hover.borrow_mut() = Some(f);
    }
}

struct ContentIvars {
    mouse: Rc<MouseState>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "OrielStripContentView"]
    #[ivars = ContentIvars]
    struct StripContentView;

    unsafe impl NSObjectProtocol for StripContentView {}

    impl StripContentView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let local = unsafe {
                match self.superview() {
                    Some(sv) => self.convertPoint_fromView(point, Some(&sv)),
                    None => point,
                }
            };
            if self.mouse_inRect(local, self.bounds()) {
                Some(self.retain().into_super())
            } else {
                None
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let local = self.convertPoint_fromView(event.locationInWindow(), None);
            let frames = self.ivars().mouse.frames.borrow();
            if let Some(index) = hit_tile(&frames, local) {
                drop(frames);
                self.ivars().mouse.fire_click(index);
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let mouse = &self.ivars().mouse;
            let mut accum = mouse.scroll_accum.get();
            let steps = scroll_steps(
                &mut accum,
                event.scrollingDeltaY(),
                event.hasPreciseScrollingDeltas(),
            );
            mouse.scroll_accum.set(accum);
            for step in steps {
                mouse.fire_scroll(step);
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let mouse = &self.ivars().mouse;
            if !mouse.hover_select.get() {
                return;
            }
            let local = self.convertPoint_fromView(event.locationInWindow(), None);
            let frames = mouse.frames.borrow();
            let Some(index) = hit_tile(&frames, local) else {
                return;
            };
            drop(frames);
            if mouse.last_hover.get() == Some(index) {
                return;
            }
            mouse.last_hover.set(Some(index));
            mouse.fire_hover(index);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let areas = self.trackingAreas();
            for area in areas {
                self.removeTrackingArea(&area);
            }
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            if !self.ivars().mouse.hover_select.get() {
                return;
            }
            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect;
            // SAFETY: owner is self (the view); userInfo is unused.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
                    options,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }
    }
);

impl StripContentView {
    fn new(mtm: MainThreadMarker, mouse: Rc<MouseState>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContentIvars { mouse });
        unsafe { msg_send![super(this), init] }
    }
}

/// The resident overlay panel. Created once and kept alive: `show` builds the
/// tile grid and orders it front, `select` moves the highlight without
/// rebuilding, `hide` orders it out.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    content: Retained<StripContentView>,
    glass: Retained<NSVisualEffectView>,
    tiles: RefCell<Vec<Retained<NSView>>>,
    selected: Cell<usize>,
    /// Which tiles are lit, parallel to `tiles` — `select` restyles from views
    /// alone and still needs to know not to wipe the light.
    lit: RefCell<Vec<bool>>,
    /// The lantern overlay per tile, kept so it can be faded rather than
    /// rebuilt, and the window's drift seed so it can be built on demand.
    lanterns: RefCell<Vec<Option<Retained<NSView>>>>,
    seeds: RefCell<Vec<u64>>,
    look: Cell<Look>,
    dark: Cell<bool>,
    /// Scale last applied by `show`, so `update_tile` rebuilds at the same size.
    scale: Cell<f64>,
    /// App icons by pid — resolving one is a process lookup, and a summon
    /// paints many tiles that share a handful of apps.
    icons: RefCell<HashMap<i32, Option<Retained<objc2_app_kit::NSImage>>>>,
    /// List column width last planned by `show` (content-driven).
    list_w: Cell<f64>,
    /// `None` hides the query row; `Some` shows it with that text (may be empty).
    query: RefCell<Option<String>>,
    query_label: RefCell<Option<Retained<NSTextField>>>,
    query_caret: RefCell<Option<Retained<NSView>>>,
    /// Extra height currently reserved for the query row (0 when hidden).
    query_space: Cell<f64>,
    /// When true, `hide` fades alpha out (~90 ms) before ordering out.
    fade_out: Cell<bool>,
    /// Bumped on every `show` / `hide` so a mid-fade re-summon cancels the pending `orderOut`.
    fade_generation: Rc<Cell<u32>>,
    mouse: Rc<MouseState>,
    /// Tile indices per row from the last `show`, top row first.
    rows: RefCell<Vec<Vec<usize>>>,
    /// Screen fixed for the open summon. Repainting must not follow focus or
    /// pointer changes while the user is still choosing a window.
    session_screen: RefCell<Option<SessionScreen>>,
}

struct SessionScreen {
    show_on: ShowOn,
    screen: Option<Retained<NSScreen>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScreenChoice {
    Indexed(usize),
    Main,
}

/// The screens each policy could pick from. Named rather than two bare
/// `Option<usize>`s: the call sites read `Found { active, pointer }`, which
/// cannot be silently mis-ordered.
#[derive(Clone, Copy, Default)]
struct Found {
    active: Option<usize>,
    pointer: Option<usize>,
}

fn screen_choice(show_on: ShowOn, found: Found, screen_count: usize) -> ScreenChoice {
    match show_on {
        ShowOn::ActiveScreen => found
            .active
            .filter(|&i| i < screen_count)
            .or((screen_count > 0).then_some(0))
            .map_or(ScreenChoice::Main, ScreenChoice::Indexed),
        ShowOn::MenubarScreen => {
            if screen_count > 0 {
                ScreenChoice::Indexed(0)
            } else {
                ScreenChoice::Main
            }
        }
        ShowOn::PointerScreen => found
            .pointer
            .filter(|&i| i < screen_count)
            .map_or(ScreenChoice::Main, ScreenChoice::Indexed),
    }
}

impl Strip {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(TILE_H, TILE_H)),
            NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setLevel(NSPopUpMenuWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHidesOnDeactivate(false);

        let mouse = Rc::new(MouseState::new());

        // The effect view has to BE the window's content view. Behind-window
        // vibrancy is drawn by the WindowServer into the window's own backing;
        // nested inside a layer-backed, corner-masked parent it composites into
        // that parent's layer instead and the blur never happens — the panel
        // just looks dimly tinted. So the glass is the root and everything else
        // lives inside it, with the rounded corners moved onto the glass.
        let glass = NSVisualEffectView::new(mtm);
        glass.setMaterial(material_for(true));
        // `BehindWindow` is what makes the WindowServer sample and blur whatever
        // the panel is floating over. Turning vibrancy off has to stop that
        // work, not just cover it up, or the switch cannot tell us whether the
        // compositing is what makes other windows flicker on summon.
        if vibrancy_off() {
            glass.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            glass.setState(NSVisualEffectState::Inactive);
        } else {
            glass.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            glass.setState(NSVisualEffectState::Active);
        }
        glass.setWantsLayer(true);
        if let Some(layer) = glass.layer() {
            layer.setCornerRadius(18.0);
            layer.setMasksToBounds(true);
            layer.setBorderWidth(1.0);
        }

        let content = StripContentView::new(mtm, Rc::clone(&mouse));
        content.setWantsLayer(true);
        content.setFrame(glass.bounds());
        content.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        if let Some(layer) = content.layer() {
            layer.setCornerRadius(18.0);
            layer.setMasksToBounds(true);
            if vibrancy_off() {
                layer
                    .setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.12, 0.94).CGColor()));
            } else {
                // The glass underneath is the background.
                layer.setBackgroundColor(None);
            }
        }
        glass.addSubview(&content);
        panel.setContentView(Some(&glass));

        Self {
            mtm,
            panel,
            content,
            glass,
            tiles: RefCell::new(Vec::new()),
            selected: Cell::new(0),
            lit: RefCell::new(Vec::new()),
            lanterns: RefCell::new(Vec::new()),
            seeds: RefCell::new(Vec::new()),
            look: Cell::new(Look::default()),
            dark: Cell::new(true),
            scale: Cell::new(SCALE_MEDIUM),
            icons: RefCell::new(HashMap::new()),
            list_w: Cell::new(LIST_MIN_W),
            query: RefCell::new(None),
            query_label: RefCell::new(None),
            query_caret: RefCell::new(None),
            query_space: Cell::new(0.0),
            fade_out: Cell::new(false),
            fade_generation: Rc::new(Cell::new(0)),
            mouse,
            rows: RefCell::new(Vec::new()),
            session_screen: RefCell::new(None),
        }
    }

    /// Called with the tile index when a tile is clicked.
    pub fn on_click(&self, f: impl FnMut(usize) + 'static) {
        *self.mouse.on_click.borrow_mut() = Some(Box::new(f));
    }

    /// Called with a delta (+1 / −1) when the strip is scrolled.
    pub fn on_scroll(&self, f: impl FnMut(i32) + 'static) {
        *self.mouse.on_scroll.borrow_mut() = Some(Box::new(f));
    }

    /// Called with the tile index the pointer moved onto; only fires when enabled.
    pub fn on_hover(&self, f: impl FnMut(usize) + 'static) {
        *self.mouse.on_hover.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_hover_select(&self, enabled: bool) {
        self.mouse.hover_select.set(enabled);
        self.panel.setAcceptsMouseMovedEvents(enabled);
        if !enabled {
            self.mouse.last_hover.set(None);
        }
        self.content.updateTrackingAreas();
    }

    /// Tile indices grouped into the rows of the layout last used by `show`,
    /// top row first. Reflects the current style and scale.
    pub fn rows(&self) -> Vec<Vec<usize>> {
        self.rows.borrow().clone()
    }

    /// Optional fade on dismiss only. Off by default; nothing ever animates in.
    pub fn set_fade_out(&self, enabled: bool) {
        self.fade_out.set(enabled);
    }

    /// Sets the presentation used by the next `show`.
    pub fn set_look(&self, look: Look) {
        self.look.set(look);
    }

    /// Fixes placement before a summon is resolved. Oriel never becomes key,
    /// so the app layer supplies the frontmost window's screen for the active
    /// policy; the UI retains the resulting `NSScreen` through every repaint.
    pub fn begin_session(&self, show_on: ShowOn, active_screen: Option<u32>) {
        let screen = self.resolve_screen(show_on, active_screen);
        *self.session_screen.borrow_mut() = Some(SessionScreen { show_on, screen });
    }

    /// Releases the summon-scoped screen so standalone uses of the strip can
    /// resolve their configured policy normally.
    pub fn end_session(&self) {
        *self.session_screen.borrow_mut() = None;
    }

    /// Shows the query row with `query` as its contents. `None` hides the row.
    pub fn set_query(&self, query: Option<&str>) {
        let was = self.query_space.get();
        {
            let mut slot = self.query.borrow_mut();
            *slot = query.map(str::to_owned);
        }
        let m = Metrics::at(self.scale.get());
        let now = if query.is_some() {
            m.query_space()
        } else {
            0.0
        };
        let delta = now - was;
        if delta.abs() > f64::EPSILON {
            let size = self.content.frame().size;
            self.panel
                .setContentSize(NSSize::new(size.width, size.height + delta));
            let origin = self.panel.frame().origin;
            self.panel
                .setFrameOrigin(NSPoint::new(origin.x, origin.y - delta / 2.0));
            self.query_space.set(now);
        }
        self.sync_query_row();
    }

    /// Builds the tiles in centered wrapping rows that always fit the screen,
    /// sizes and centers the panel, highlights `selected`, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
        // Cancel any in-flight fade and restore full opacity before painting —
        // a fast re-summon must never show a half-faded strip.
        self.fade_generation
            .set(self.fade_generation.get().wrapping_add(1));
        self.panel.setAlphaValue(1.0);

        for old in self.tiles.borrow().iter() {
            old.removeFromSuperview();
        }

        let look = self.look.get();
        let screen = self.target_screen(look.show_on);
        let (screen_w, screen_h, frame) = match &screen {
            Some(s) => {
                let vf = s.visibleFrame();
                (vf.size.width, vf.size.height, Some(vf))
            }
            None => (1440.0, 900.0, None),
        };

        *self.lit.borrow_mut() = tiles.iter().map(|t| t.lantern).collect();
        *self.seeds.borrow_mut() = tiles.iter().map(drift_seed).collect();
        self.lanterns.borrow_mut().clear();
        self.lanterns.borrow_mut().resize_with(tiles.len(), || None);
        let aspects: Vec<f64> = tiles.iter().map(|t| t.aspect).collect();
        let scale = size_scale(
            look.size,
            look.style,
            tiles.len(),
            &aspects,
            screen_w,
            screen_h,
            self.query_space.get(),
        );
        self.scale.set(scale);
        let m = Metrics::at(scale);
        let dark = theme_is_dark(look.theme);
        self.dark.set(dark);
        self.apply_theme(dark);

        let query_space = if self.query.borrow().is_some() {
            m.query_space()
        } else {
            0.0
        };
        self.query_space.set(query_space);
        if look.style == Style::List {
            self.list_w
                .set(list_content_width(tiles, &m, look.title_show, look.markers));
        }
        let plan = plan(
            tiles,
            look.style,
            &m,
            Screen {
                w: screen_w,
                h: screen_h,
            },
            query_space,
            look.title_show,
            look.markers,
        );
        self.panel
            .setContentSize(NSSize::new(plan.width, plan.height));

        let mut views = Vec::with_capacity(tiles.len());
        let mut frames = Vec::with_capacity(tiles.len());
        for (i, tile) in tiles.iter().enumerate() {
            let view = self.tile(tile, look.style, &m, dark);
            // `select` only restyles the two tiles it moves between, so a lit
            // window that is not the selection has to be lit here.
            if tile.lantern {
                set_highlight(
                    &view,
                    Emphasis {
                        selected: false,
                        lit: true,
                        dark,
                    },
                );
                let glass = self.lantern_glass(view.bounds(), drift_seed(tile));
                view.addSubview(&glass);
                if let Some(slot) = self.lanterns.borrow_mut().get_mut(i) {
                    *slot = Some(glass);
                }
            }
            let (x, y) = plan.origins[i];
            view.setFrameOrigin(NSPoint::new(x, y));
            frames.push(view.frame());
            self.content.addSubview(&view);
            views.push(view);
        }
        self.tiles.replace(views);
        *self.mouse.frames.borrow_mut() = frames;
        *self.rows.borrow_mut() = plan.rows;
        self.mouse.scroll_accum.set(0.0);
        self.mouse.last_hover.set(None);
        self.selected.set(usize::MAX);
        self.select(selected);
        self.sync_query_row();

        // AppKit's center() floats windows above the visual middle; the strip
        // sits at the true center of the visible screen instead.
        if let Some(frame) = frame {
            self.panel.setFrameOrigin(NSPoint::new(
                (frame.size.width - plan.width).mul_add(0.5, frame.origin.x),
                (frame.size.height - plan.height).mul_add(0.5, frame.origin.y),
            ));
        } else {
            self.panel.center();
        }
        self.panel.orderFrontRegardless();
    }

    /// Turns a tile's lantern on or off, fading rather than switching.
    ///
    /// An agent stopping is not an event worth flinching at — the mark should
    /// ebb the way the work did. Rebuilding the tile, which is how a lit state
    /// used to change, gave a hard cut and threw away the drift's phase with it.
    pub fn set_lantern(&self, index: usize, on: bool) {
        if self.lit.borrow().get(index).copied() == Some(on) {
            return;
        }
        let tiles = self.tiles.borrow();
        let Some(view) = tiles.get(index) else {
            return;
        };
        if let Some(slot) = self.lit.borrow_mut().get_mut(index) {
            *slot = on;
        }

        let existing = self.lanterns.borrow().get(index).and_then(Clone::clone);
        let glass = match existing {
            Some(glass) => glass,
            None if on => {
                let seed = self.seeds.borrow().get(index).copied().unwrap_or_default();
                let glass = self.lantern_glass(view.bounds(), seed);
                glass.setAlphaValue(0.0);
                view.addSubview(&glass);
                if let Some(slot) = self.lanterns.borrow_mut().get_mut(index) {
                    *slot = Some(glass.clone());
                }
                glass
            }
            None => return,
        };

        fade_view(&glass, if on { 1.0 } else { 0.0 }, LANTERN_FADE);
        if !on {
            // Transparent is not gone. Four gradient layers, each with two
            // endless animations, would go on being composited behind a fully
            // faded view for the rest of the session. Drop it once the fade has
            // played; an agent that starts again is given a fresh one.
            self.lanterns.borrow_mut().get_mut(index).map(Option::take);
            let done = Box::new(LanternDone {
                glass,
                generation: Rc::clone(&self.fade_generation),
                stamp: self.fade_generation.get(),
            });
            let when = unsafe { dispatch_time(0, LANTERN_FADE_NS) };
            // SAFETY: paired with `Box::from_raw` in `lantern_fade_finished`.
            unsafe {
                dispatch_after_f(
                    when,
                    core::ptr::from_ref(&_dispatch_main_q).cast(),
                    Box::into_raw(done).cast(),
                    lantern_fade_finished,
                );
            }
        }
        set_highlight(
            view,
            Emphasis {
                selected: index == self.selected.get(),
                lit: on,
                dark: self.dark.get(),
            },
        );
    }

    /// Moves the highlight to `index` by restyling two tiles — the cheap path
    /// taken on every cycle, so it keeps pace with key-repeat.
    pub fn select(&self, index: usize) {
        let tiles = self.tiles.borrow();
        if index >= tiles.len() || index == self.selected.get() {
            return;
        }
        let dark = self.dark.get();
        let lit = self.lit.borrow();
        let is_lit = |i: usize| lit.get(i).copied().unwrap_or(false);
        if let Some(old) = tiles.get(self.selected.get()) {
            set_highlight(
                old,
                Emphasis {
                    selected: false,
                    lit: is_lit(self.selected.get()),
                    dark,
                },
            );
        }
        set_highlight(
            &tiles[index],
            Emphasis {
                selected: true,
                lit: is_lit(index),
                dark,
            },
        );
        self.selected.set(index);
    }

    /// Rebuilds one tile in place — the path a freshly captured preview takes
    /// while the strip is up. Keeps the frame and the selection highlight.
    pub fn update_tile(&self, index: usize, tile: &Tile) {
        let mut tiles = self.tiles.borrow_mut();
        let Some(slot) = tiles.get_mut(index) else {
            return;
        };
        let look = self.look.get();
        let m = Metrics::at(self.scale.get());
        let dark = self.dark.get();
        let view = self.tile(tile, look.style, &m, dark);
        view.setFrameOrigin(slot.frame().origin);
        let frame = view.frame();
        self.content.addSubview(&view);
        slot.removeFromSuperview();
        // A preview arriving decides nothing about the lantern. Rebuilding the
        // overlay here restarted its drift from the beginning and cut past the
        // fade — and captures land throughout a session, so a working window's
        // shimmer kept snapping back to the same phase. `set_lantern` owns it;
        // this only carries what already exists onto the new view.
        let lit = self.lit.borrow().get(index).copied().unwrap_or(false);
        set_highlight(
            &view,
            Emphasis {
                selected: index == self.selected.get(),
                lit,
                dark,
            },
        );
        if let Some(glass) = self.lanterns.borrow().get(index).and_then(Clone::clone) {
            glass.removeFromSuperview();
            glass.setFrame(view.bounds());
            view.addSubview(&glass);
        }
        *slot = view;
        if let Some(cached) = self.mouse.frames.borrow_mut().get_mut(index) {
            *cached = frame;
        }
    }

    pub fn hide(&self) {
        let stamp = self.fade_generation.get().wrapping_add(1);
        self.fade_generation.set(stamp);
        self.mouse.last_hover.set(None);
        self.mouse.scroll_accum.set(0.0);
        if !self.fade_out.get() {
            self.panel.setAlphaValue(1.0);
            self.panel.orderOut(None);
            return;
        }
        // SAFETY: NSAnimationContext begin/endGrouping + animator alpha ramp.
        unsafe {
            let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
            let ctx: Retained<AnyObject> = msg_send![class!(NSAnimationContext), currentContext];
            let _: () = msg_send![&*ctx, setDuration: FADE_OUT_SECS];
            let _: () = msg_send![&*ctx, setAllowsImplicitAnimation: true];
            let animator: Retained<AnyObject> = msg_send![&*self.panel, animator];
            let _: () = msg_send![&*animator, setAlphaValue: 0.0_f64];
            let _: () = msg_send![class!(NSAnimationContext), endGrouping];
        }
        let done = Box::new(FadeDone {
            panel: self.panel.clone(),
            generation: Rc::clone(&self.fade_generation),
            stamp,
        });
        let when = unsafe { dispatch_time(0, FADE_OUT_NS) };
        unsafe {
            dispatch_after_f(
                when,
                core::ptr::from_ref(&_dispatch_main_q).cast(),
                Box::into_raw(done).cast(),
                fade_out_finished,
            );
        }
    }

    /// Vibrancy carries the background, so the theme only sets the hairline
    /// edge that separates the panel from whatever it is floating over.
    fn apply_theme(&self, dark: bool) {
        if let Some(layer) = self.glass.layer() {
            let edge = if dark {
                NSColor::colorWithWhite_alpha(1.0, 0.28)
            } else {
                NSColor::colorWithWhite_alpha(0.0, 0.18)
            };
            layer.setBorderColor(Some(&edge.CGColor()));
        }
        self.glass.setMaterial(material_for(dark));
    }

    fn target_screen(&self, show_on: ShowOn) -> Option<Retained<NSScreen>> {
        if let Some(cached) = self.session_screen.borrow().as_ref()
            && cached.show_on == show_on
        {
            return cached.screen.clone();
        }
        self.resolve_screen(show_on, None)
    }

    fn resolve_screen(
        &self,
        show_on: ShowOn,
        active_screen: Option<u32>,
    ) -> Option<Retained<NSScreen>> {
        let screens = NSScreen::screens(self.mtm);
        let count = screens.count();
        let pointer = if show_on == ShowOn::PointerScreen {
            let loc = mouse_location();
            (0..screens.count()).find(|&i| {
                let screen = screens.objectAtIndex(i);
                NSPointInRect(loc, screen.frame())
            })
        } else {
            None
        };
        let found = Found {
            active: active_screen.and_then(|i| usize::try_from(i).ok()),
            pointer,
        };
        match screen_choice(show_on, found, count) {
            ScreenChoice::Indexed(i) => Some(screens.objectAtIndex(i)),
            ScreenChoice::Main => NSScreen::mainScreen(self.mtm),
        }
    }

    /// App icon for `pid`, memoised for the process's lifetime. Several windows
    /// usually share one app, and summon must not repeat the process lookup per
    /// tile (PRD §5.5).
    fn app_icon(&self, pid: i32) -> Option<Retained<objc2_app_kit::NSImage>> {
        if let Some(hit) = self.icons.borrow().get(&pid) {
            return hit.clone();
        }
        let icon = app_icon_uncached(pid);
        self.icons.borrow_mut().insert(pid, icon.clone());
        icon
    }

    /// Index of the screen `show_on` selects, in `NSScreen::screens` order —
    /// so the lens resolver's `strip-screen` scope filters on the screen the
    /// strip will actually appear on.
    pub fn screen_index(&self, show_on: ShowOn) -> u32 {
        let Some(target) = self.target_screen(show_on) else {
            return 0;
        };
        let screens = NSScreen::screens(self.mtm);
        for i in 0..screens.count() {
            if screens.objectAtIndex(i) == target {
                return u32::try_from(i).unwrap_or(0);
            }
        }
        0
    }

    fn tile(&self, tile: &Tile, style: Style, m: &Metrics, dark: bool) -> Retained<NSView> {
        match style {
            Style::Gallery => self.gallery_tile(tile, m, dark),
            Style::Icons => self.icons_tile(tile, m, dark),
            Style::List => self.list_tile(tile, m, dark),
        }
    }

    /// The light on the glass: a dim warm gradient laid over the whole tile,
    /// drifting slowly so a working window reads as alive without ever asking
    /// to be looked at. The corner radius clips it to the tile's shape.
    ///
    /// This is the one thing in the strip that moves. It earns it: stillness is
    /// what tells you a window is finished, so the difference has to be motion.
    fn lantern_glass(&self, bounds: NSRect, seed: u64) -> Retained<NSView> {
        let glass = NSView::initWithFrame(self.mtm.alloc(), bounds);
        glass.setWantsLayer(true);
        let Some(host) = glass.layer() else {
            return glass;
        };
        host.setMasksToBounds(true);
        for field in &LANTERN_FIELDS {
            host.addSublayer(&lantern_field(field, bounds, seed));
        }
        glass
    }
}

/// One drifting field of colour.
fn lantern_field(field: &Field, bounds: NSRect, seed: u64) -> Retained<CAGradientLayer> {
    let reach = bounds.size.width.max(bounds.size.height);
    let span = reach * field.size;
    let (wide, tall) = (span * field.stretch, span / field.stretch);
    let blob = CAGradientLayer::new();
    blob.setFrame(NSRect::new(
        NSPoint::new(
            bounds
                .size
                .width
                .mul_add(0.5 + field.anchor.0, -(wide / 2.0)),
            bounds
                .size
                .height
                .mul_add(0.5 + field.anchor.1, -(tall / 2.0)),
        ),
        NSSize::new(wide, tall),
    ));

    let (red, green, blue) = field.rgb;
    let colors: Retained<AnyObject> = unsafe { msg_send![class!(NSMutableArray), array] };
    for color in [
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, field.alpha),
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, field.alpha * 0.55),
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 0.0),
    ] {
        let cg = color.CGColor();
        let ptr: *const AnyObject = (&raw const *cg).cast();
        let _: () = unsafe { msg_send![&*colors, addObject: ptr] };
    }
    blob.setLocations(Some(&stops(&[0.0, 0.55, 1.0])));
    unsafe {
        let _: () = msg_send![&*blob, setColors: &*colors];
        let _: () = msg_send![&*blob, setType: &*NSString::from_str("radial")];
        let _: () = msg_send![&*blob, setStartPoint: NSPoint::new(0.5, 0.5)];
        let _: () = msg_send![&*blob, setEndPoint: NSPoint::new(1.0, 1.0)];
        // A radial gradient fills its layer's box, so an oblong layer gives
        // an ellipse; turning each one differently is what keeps four
        // overlapping fields from resolving into a recognisable shape.
        let _: () = msg_send![
            &*blob,
            setValue: &*NSNumber::new_f64(field.tilt),
            forKeyPath: &*NSString::from_str("transform.rotation.z")
        ];
        // Blended as light, not as paint. A translucent colour over a dark
        // preview composites toward grey — the chroma is crushed by the very
        // background it mixes with, which is why saturated stops still
        // arrived as a white haze. Screen blending adds instead of mixing,
        // so the hue survives on dark pixels while bright parts of the
        // preview stay readable.
        let _: () = msg_send![
            &*blob,
            setCompositingFilter: &*NSString::from_str("screenBlendMode")
        ];
    }

    for (index, (axis, seconds)) in [("x", field.periods.0), ("y", field.periods.1)]
        .into_iter()
        .enumerate()
    {
        // Every lit window builds the same fields in the same frame, so
        // without a phase of its own each window would drift in lockstep
        // with the rest and read as one effect stamped across the strip.
        // The seed comes from the window itself, so a window keeps its
        // phase across re-renders rather than jumping every time it is
        // drawn.
        let scrambled = seed.wrapping_mul(if index == 0 {
            0x5851_F42D_4C95_7F2D
        } else {
            0x1405_7B7E_F767_814F
        });
        #[allow(clippy::cast_precision_loss)]
        let offset = (scrambled >> 11) as f64 / (1u64 << 53) as f64;
        let seconds = seconds * (0.82 + offset * 0.36);
        let travel = reach * field.sway;
        let path = format!("transform.translation.{axis}");
        let slide = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(&path)));
        // SAFETY: a translation component takes a plain number.
        unsafe {
            slide.setFromValue(Some(&NSNumber::new_f64(-travel)));
            slide.setToValue(Some(&NSNumber::new_f64(travel)));
            slide.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(
                &NSString::from_str("easeInEaseOut"),
            )));
        }
        slide.setTimeOffset(offset * seconds);
        slide.setDuration(seconds);
        slide.setAutoreverses(true);
        slide.setRepeatCount(f32::INFINITY);
        blob.addAnimation_forKey(&slide, Some(&NSString::from_str(&path)));
    }
    blob
}

impl Strip {
    /// Gallery: caption row on top, preview (or large icon) below.
    fn gallery_tile(&self, tile: &Tile, m: &Metrics, dark: bool) -> Retained<NSView> {
        let width = gallery_tile_width(tile.aspect, m);
        let height = m.gallery_tile_h();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(10.0);
            layer.setMasksToBounds(true);
        }

        // Caption sits above the preview, matching the original Gallery layout.
        let caption_base = height - 4.0 - m.caption_h;
        self.caption_row(&view, tile, width, caption_base, m.caption_h, m, dark, true);
        let surface = match &tile.preview {
            Some(image) => self.preview_surface(image, width, m),
            None => self.icon_surface(tile.pid, width, m),
        };
        view.addSubview(&surface);
        view
    }

    /// Icons: large app icon with the title beneath; uniform content-hugging tile.
    fn icons_tile(&self, tile: &Tile, m: &Metrics, dark: bool) -> Retained<NSView> {
        let width = m.icons_tile_w();
        let height = m.icons_tile_h();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(10.0);
            layer.setMasksToBounds(true);
        }

        let icon_s = m.icons_icon;
        if let Some(icon) = self.app_icon(tile.pid) {
            let icon_y = m.inset + m.caption_h + ICONS_GAP;
            let frame = NSRect::new(
                NSPoint::new((width - icon_s) / 2.0, icon_y),
                NSSize::new(icon_s, icon_s),
            );
            view.addSubview(&scaled_icon_view(self.mtm, &icon, frame));
        }

        self.caption_row(&view, tile, width, m.inset, m.caption_h, m, dark, false);
        view
    }

    /// List: dense row — small icon, title, markers right-aligned.
    fn list_tile(&self, tile: &Tile, m: &Metrics, dark: bool) -> Retained<NSView> {
        let width = self.list_w.get();
        let height = m.list_row_h();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(6.0);
            layer.setMasksToBounds(true);
        }

        let pad_y = m.list_pad_y;
        let band = height - 2.0 * pad_y;
        self.caption_row(&view, tile, width, pad_y, band, m, dark, true);
        view
    }

    /// Shared caption: optional small app icon, title, right-aligned badges.
    /// `with_icon` is false for Icons style (the large icon already identifies
    /// the app). `base` is the bottom edge of the caption band; `band_h` its height.
    #[allow(clippy::too_many_arguments)]
    fn caption_row(
        &self,
        view: &NSView,
        tile: &Tile,
        width: f64,
        base: f64,
        band_h: f64,
        m: &Metrics,
        dark: bool,
        with_icon: bool,
    ) {
        let title_color = if dark {
            NSColor::colorWithWhite_alpha(1.0, 0.9)
        } else {
            NSColor::colorWithWhite_alpha(0.1, 0.9)
        };
        let badge_color = if dark {
            NSColor::colorWithWhite_alpha(1.0, 0.75)
        } else {
            NSColor::colorWithWhite_alpha(0.25, 0.75)
        };

        let mut text_x = m.inset;
        if with_icon && let Some(icon) = self.app_icon(tile.pid) {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            let icon_s = m.caption_icon;
            let icon_y = base + (band_h - icon_s).max(0.0) / 2.0;
            image.setFrame(NSRect::new(
                NSPoint::new(m.inset, icon_y),
                NSSize::new(icon_s, icon_s),
            ));
            view.addSubview(&image);
            text_x += icon_s + 6.0;
        }

        let mut text_end = width - m.inset;
        let look = self.look.get();
        if look.markers && !tile.badge.is_empty() {
            let marks = NSTextField::labelWithString(&NSString::from_str(&tile.badge), self.mtm);
            marks.setFont(Some(&NSFont::systemFontOfSize(m.badge_font)));
            marks.setTextColor(Some(&badge_color));
            marks.sizeToFit();
            let w = marks.frame().size.width;
            let mark_y = base + (band_h - m.badge_font - 2.0).max(0.0) / 2.0;
            marks.setFrameOrigin(NSPoint::new(width - m.inset - w, mark_y));
            view.addSubview(&marks);
            text_end -= w + 6.0;
        }

        let (text, spans) = caption_parts(tile, look.title_show);
        let label = attributed_label(
            &text,
            &spans,
            m.title_font,
            &title_color,
            look.title_truncate,
            self.mtm,
        );
        label.setMaximumNumberOfLines(1);
        let label_h = m.title_font + 5.0;
        let label_y = base + (band_h - label_h).max(0.0) / 2.0;
        label.setFrame(NSRect::new(
            NSPoint::new(text_x, label_y),
            NSSize::new((text_end - text_x).max(10.0), label_h),
        ));
        view.addSubview(&label);
    }

    fn sync_query_row(&self) {
        let m = Metrics::at(self.scale.get());
        let dark = self.dark.get();
        let query = self.query.borrow().clone();
        let Some(text) = query else {
            if let Some(label) = self.query_label.borrow_mut().take() {
                label.removeFromSuperview();
            }
            if let Some(caret) = self.query_caret.borrow_mut().take() {
                caret.removeFromSuperview();
            }
            return;
        };

        let panel_w = self.content.frame().size.width;
        let panel_h = self.content.frame().size.height;
        let row_y = panel_h - m.pad - m.query_h;
        let content_w = (panel_w - 2.0 * m.pad).max(10.0);

        let prompt = text.is_empty();
        let display = if prompt { QUERY_PROMPT } else { text.as_str() };
        let color = if prompt {
            if dark {
                NSColor::colorWithWhite_alpha(1.0, 0.35)
            } else {
                NSColor::colorWithWhite_alpha(0.0, 0.35)
            }
        } else if dark {
            NSColor::colorWithWhite_alpha(1.0, 0.92)
        } else {
            NSColor::colorWithWhite_alpha(0.08, 0.92)
        };

        let label = {
            let mut slot = self.query_label.borrow_mut();
            if let Some(existing) = slot.as_ref() {
                existing.clone()
            } else {
                let created = NSTextField::labelWithString(&NSString::from_str(display), self.mtm);
                self.content.addSubview(&created);
                *slot = Some(created.clone());
                created
            }
        };
        label.setStringValue(&NSString::from_str(display));
        label.setFont(Some(&NSFont::systemFontOfSize(m.query_font)));
        label.setTextColor(Some(&color));
        label.setMaximumNumberOfLines(1);
        label.sizeToFit();
        let text_h = m.query_font + 5.0;
        let text_y = row_y + (m.query_h - text_h).max(0.0) / 2.0;
        let caret_h = (m.query_font + 2.0).min(m.query_h - 4.0);
        let caret_y = row_y + (m.query_h - caret_h).max(0.0) / 2.0;
        let text_w = if prompt {
            0.0
        } else {
            label.frame().size.width.min(content_w - CARET_W - 4.0)
        };
        let caret_x = m.pad + text_w + if prompt { 0.0 } else { 1.0 };
        let label_x = if prompt {
            caret_x + CARET_W + 3.0
        } else {
            m.pad
        };
        let label_w = if prompt {
            (content_w - (label_x - m.pad)).max(10.0)
        } else {
            text_w.max(10.0)
        };
        label.setFrame(NSRect::new(
            NSPoint::new(label_x, text_y),
            NSSize::new(label_w, text_h),
        ));

        let caret = {
            let mut slot = self.query_caret.borrow_mut();
            if let Some(existing) = slot.as_ref() {
                existing.clone()
            } else {
                let created = NSView::initWithFrame(
                    self.mtm.alloc(),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CARET_W, caret_h)),
                );
                created.setWantsLayer(true);
                self.content.addSubview(&created);
                *slot = Some(created.clone());
                created
            }
        };
        if let Some(layer) = caret.layer() {
            layer.setBackgroundColor(Some(&NSColor::controlAccentColor().CGColor()));
        }
        caret.setFrame(NSRect::new(
            NSPoint::new(caret_x, caret_y),
            NSSize::new(CARET_W, caret_h),
        ));
    }

    fn surface_frame(width: f64, m: &Metrics) -> NSRect {
        NSRect::new(
            NSPoint::new(m.inset, m.inset),
            NSSize::new(width - 2.0 * m.inset, m.preview_h),
        )
    }

    /// The whole window, aspect-fit inside the preview box.
    fn preview_surface(&self, image: &CGImage, width: f64, m: &Metrics) -> Retained<NSView> {
        let host = NSView::initWithFrame(self.mtm.alloc(), Self::surface_frame(width, m));
        host.setWantsLayer(true);
        if let Some(layer) = host.layer() {
            let contents: &AnyObject = unsafe { &*core::ptr::from_ref(image).cast::<AnyObject>() };
            unsafe { layer.setContents(Some(contents)) };
            layer.setContentsGravity(unsafe { kCAGravityResizeAspect });
            layer.setCornerRadius(6.0);
            layer.setMasksToBounds(true);
        }
        host
    }

    /// Fallback: a large centered app icon in place of the preview.
    fn icon_surface(&self, pid: i32, width: f64, m: &Metrics) -> Retained<NSView> {
        let host = NSView::initWithFrame(self.mtm.alloc(), Self::surface_frame(width, m));
        if let Some(icon) = self.app_icon(pid) {
            let frame = NSRect::new(
                NSPoint::new(
                    (width - 2.0 * m.inset - m.icon) / 2.0,
                    (m.preview_h - m.icon) / 2.0,
                ),
                NSSize::new(m.icon, m.icon),
            );
            host.addSubview(&scaled_icon_view(self.mtm, &icon, frame));
        }
        host
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact arithmetic on layout constants
mod tests {
    use super::*;

    #[test]
    fn width_follows_aspect_within_bounds() {
        assert_eq!(tile_width(1.0), PREVIEW_H + 2.0 * INSET);
        assert_eq!(tile_width(0.2), MIN_W + 2.0 * INSET);
        assert_eq!(tile_width(10.0), MAX_W + 2.0 * INSET);
    }

    #[test]
    fn single_tile_layout() {
        let plan = layout(&[100.0], 1000.0);
        assert_eq!(plan.origins, [(PAD, PAD)]);
        assert_eq!(plan.width, 100.0 + 2.0 * PAD);
        assert_eq!(plan.height, TILE_H + 2.0 * PAD);
    }

    #[test]
    fn wraps_when_a_row_is_full() {
        let plan = layout(&[100.0, 100.0, 100.0], 220.0);
        // two rows: [0, 1] then [2]
        assert_eq!(plan.height, 2.0 * TILE_H + GAP + 2.0 * PAD);
        assert_eq!(plan.width, 200.0 + GAP + 2.0 * PAD);
        assert!(plan.origins[0].1 > plan.origins[2].1);
    }

    #[test]
    fn short_rows_are_centered() {
        let plan = layout(&[100.0, 100.0, 100.0], 220.0);
        let lone = plan.origins[2].0;
        assert!(lone > plan.origins[0].0);
        assert!((lone - (PAD + (200.0 + GAP - 100.0) / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn rows_balance_instead_of_stuffing_the_first() {
        // six equal tiles, the hard limit fits four per row: balanced 3+3
        let plan = layout(&[200.0; 6], 1000.0);
        let bottom = plan.origins[5].1;
        assert_eq!(plan.origins.iter().filter(|o| o.1 > bottom).count(), 3);
    }

    #[test]
    fn oversized_tile_still_gets_a_row() {
        let plan = layout(&[500.0, 100.0], 400.0);
        assert!(plan.origins[0].1 > plan.origins[1].1);
        assert_eq!(plan.width, 500.0 + 2.0 * PAD);
    }

    #[test]
    fn look_default_is_todays_behaviour() {
        let look = Look::default();
        assert_eq!(look.style, Style::Gallery);
        assert_eq!(look.size, Size::Auto);
        assert_eq!(look.show_on, ShowOn::ActiveScreen);
        assert_eq!(look.theme, Theme::System);
        assert_eq!(look.title_show, TitleShow::Title);
        assert_eq!(look.title_truncate, TitleTruncate::End);
        assert!(look.markers);
    }

    #[test]
    fn active_screen_prefers_focused_window_then_menubar() {
        assert_eq!(
            screen_choice(
                ShowOn::ActiveScreen,
                Found {
                    active: Some(2),
                    pointer: None
                },
                3
            ),
            ScreenChoice::Indexed(2)
        );
        assert_eq!(
            screen_choice(ShowOn::ActiveScreen, Found::default(), 3),
            ScreenChoice::Indexed(0)
        );
        assert_eq!(
            screen_choice(
                ShowOn::ActiveScreen,
                Found {
                    active: Some(9),
                    pointer: None
                },
                3
            ),
            ScreenChoice::Indexed(0)
        );
        assert_eq!(
            screen_choice(ShowOn::ActiveScreen, Found::default(), 0),
            ScreenChoice::Main
        );
    }

    #[test]
    fn pointer_and_menubar_screen_policies_keep_their_fallbacks() {
        assert_eq!(
            screen_choice(
                ShowOn::PointerScreen,
                Found {
                    active: None,
                    pointer: Some(1)
                },
                2
            ),
            ScreenChoice::Indexed(1)
        );
        assert_eq!(
            screen_choice(ShowOn::PointerScreen, Found::default(), 2),
            ScreenChoice::Main
        );
        assert_eq!(
            screen_choice(
                ShowOn::MenubarScreen,
                Found {
                    active: Some(1),
                    pointer: Some(1)
                },
                2
            ),
            ScreenChoice::Indexed(0)
        );
    }

    #[test]
    fn medium_metrics_match_legacy_constants() {
        let m = Metrics::at(SCALE_MEDIUM);
        assert_eq!(m.preview_h, PREVIEW_H);
        assert_eq!(m.caption_h, CAPTION_H);
        assert_eq!(m.icon, ICON);
        assert_eq!(m.icons_icon, ICONS_ICON);
        assert_eq!(m.min_w, MIN_W);
        assert_eq!(m.max_w, MAX_W);
        assert_eq!(m.gap, GAP);
        assert_eq!(m.pad, PAD);
        assert_eq!(m.inset, INSET);
        assert_eq!(m.gallery_tile_h(), TILE_H);
    }

    #[test]
    fn icons_tile_hugs_large_icon() {
        let m = Metrics::at(SCALE_MEDIUM);
        assert!((96.0..129.0).contains(&m.icons_icon));
        assert_eq!(m.icons_tile_w(), m.icons_icon + 2.0 * m.inset);
        assert_eq!(
            m.icons_tile_h(),
            m.icons_icon + ICONS_GAP + m.caption_h + 2.0 * m.inset
        );
        // Uniform grid: width follows the icon, not Gallery's preview box.
        assert!(m.icons_tile_w() < m.gallery_tile_h());
        assert!(m.icons_tile_w() < gallery_tile_width(1.5, &m));
    }

    #[test]
    fn list_rows_are_dense() {
        let m = Metrics::at(SCALE_MEDIUM);
        let row = m.list_row_h();
        // Menu pitch — not Gallery tile height, not the old inset*2 padding.
        assert!(row < 40.0);
        assert!(row < m.caption_h + 2.0 * m.inset);
        assert_eq!(m.list_gap, LIST_GAP);
        let tiles = [
            Tile {
                title: "Short".into(),
                ..Tile::default()
            },
            Tile {
                title: "A rather longer window title here".into(),
                badge: "●".into(),
                ..Tile::default()
            },
        ];
        let short = list_content_width(&tiles[..1], &m, TitleShow::Title, true);
        let long = list_content_width(&tiles, &m, TitleShow::Title, true);
        assert!(long > short);
        assert!(long <= m.list_max_w);
        assert!(short >= m.list_min_w);
        let plan = list_layout(9, long, row, m.list_gap, m.list_pad, 0.0, 900.0);
        // Nine dense rows fit a laptop-height screen with room to spare.
        assert!(plan.height < 900.0);
    }

    #[test]
    fn fixed_sizes_are_a_ceiling_that_still_fits() {
        // A fixed size must never make the strip overflow: it caps the scale,
        // and the layout still densifies below it when there are many windows.
        for style in [Style::Gallery, Style::Icons, Style::List] {
            for (size, ceiling) in [
                (Size::Small, SCALE_SMALL),
                (Size::Medium, SCALE_MEDIUM),
                (Size::Large, SCALE_LARGE),
            ] {
                for &n in &[1usize, 8, 40, 120] {
                    let s = size_scale(size, style, n, &[], 1440.0, 900.0, 0.0);
                    assert!(s <= ceiling, "{style:?} {size:?} n={n}: {s} above ceiling");
                    assert!(
                        panel_fits(style, n, &[], 1440.0, 900.0, s, 0.0),
                        "{style:?} {size:?} n={n}: scale {s} overflows"
                    );
                }
            }
        }
    }

    #[test]
    fn the_caption_band_always_fits_what_it_draws() {
        // Gallery and Icons both size their tiles from `caption_h`; if it can
        // fall below the clamped icon/font the caption spills out of the tile.
        let mut s = 1.6;
        while s > 0.01 {
            let m = Metrics::at(s);
            let drawn = m.caption_icon.max(m.title_font + 5.0);
            assert!(
                m.caption_h >= drawn,
                "caption band {} shorter than its contents {} at scale {s}",
                m.caption_h,
                drawn
            );
            s -= 0.01;
        }
    }

    #[test]
    fn a_list_row_always_fits_what_it_draws() {
        // The caption's icon and font are clamped, so the row must be measured
        // from the clamped values, not from raw scale.
        let mut s = 1.6;
        while s > 0.01 {
            let m = Metrics::at(s);
            let drawn = m.caption_icon.max(m.title_font + 5.0);
            assert!(
                m.list_row_h() >= drawn,
                "row {} shorter than its contents {} at scale {s}",
                m.list_row_h(),
                drawn
            );
            s -= 0.01;
        }
    }

    #[test]
    fn list_width_bounds_never_invert() {
        // `f64::clamp` panics if min > max. List densifies far below the
        // readability floor, so walk every scale the ladder can pick.
        let mut s = 1.6;
        while s > 0.01 {
            let m = Metrics::at(s);
            assert!(
                m.list_min_w <= m.list_max_w,
                "inverted at scale {s}: min={} max={}",
                m.list_min_w,
                m.list_max_w
            );
            s -= 0.01;
        }
    }

    #[test]
    fn a_crowded_list_lays_out_without_panicking() {
        // The reachable path: List + Auto + many windows on a short screen.
        for &n in &[1usize, 40, 120, 300] {
            let scale = size_scale(Size::Auto, Style::List, n, &[], 1280.0, 700.0, 0.0);
            let m = Metrics::at(scale);
            let tiles: Vec<Tile> = (0..n)
                .map(|i| Tile {
                    title: format!("window {i} with a reasonably long title"),
                    ..Tile::default()
                })
                .collect();
            let w = list_content_width(&tiles, &m, TitleShow::Title, true);
            assert!(w.is_finite() && w > 0.0, "n={n} produced width {w}");
        }
    }

    #[test]
    fn wide_windows_shrink_the_scale_more_than_nominal_ones() {
        // Probing with one nominal shape under-measures a row of ultrawides and
        // lets the panel run off the screen.
        let wide = vec![4.0; 12];
        let nominal = vec![NOMINAL_ASPECT; 12];
        let s_wide = size_scale(Size::Auto, Style::Gallery, 12, &wide, 1440.0, 900.0, 0.0);
        let s_nom = size_scale(Size::Auto, Style::Gallery, 12, &nominal, 1440.0, 900.0, 0.0);
        assert!(
            s_wide <= s_nom,
            "wide {s_wide} should not exceed nominal {s_nom}"
        );
        assert!(panel_fits(
            Style::Gallery,
            12,
            &wide,
            1440.0,
            900.0,
            s_wide,
            0.0
        ));
    }

    #[test]
    fn a_small_screen_shrinks_a_large_size() {
        // Same window count, Large requested: the smaller screen must not get
        // the same scale as the roomy one.
        let big = size_scale(Size::Large, Style::Gallery, 30, &[], 2560.0, 1440.0, 0.0);
        let small = size_scale(Size::Large, Style::Gallery, 30, &[], 1280.0, 800.0, 0.0);
        assert!(small <= big);
    }

    #[test]
    fn hiding_markers_narrows_list_column() {
        let m = Metrics::at(SCALE_MEDIUM);
        let tiles = [Tile {
            title: "A rather longer window title here".into(),
            badge: "● 2".into(),
            ..Tile::default()
        }];
        let with = list_content_width(&tiles, &m, TitleShow::Title, true);
        let without = list_content_width(&tiles, &m, TitleShow::Title, false);
        assert!(with > without);
        // Title regains the badge slot; width must still clamp to list bounds.
        assert!(without >= m.list_min_w);
        assert!(with <= m.list_max_w);
    }

    #[test]
    fn caption_parts_respect_show_mode() {
        let tile = Tile {
            app: "Safari".into(),
            title: "Home".into(),
            app_spans: vec![(0, 3)],
            title_spans: vec![(0, 2)],
            ..Tile::default()
        };
        let (as_title, spans) = caption_parts(&tile, TitleShow::Title);
        assert_eq!(as_title, "Home");
        assert_eq!(spans, vec![(0, 2)]);

        let (as_app, spans) = caption_parts(&tile, TitleShow::App);
        assert_eq!(as_app, "Safari");
        assert_eq!(spans, vec![(0, 3)]);

        let (as_both, spans) = caption_parts(&tile, TitleShow::Both);
        assert_eq!(as_both, format!("Safari{BOTH_SEP}Home"));
        let title_offset = "Safari".len() + BOTH_SEP.len();
        assert_eq!(spans, vec![(0, 3), (title_offset, 2)]);

        let empty_title = Tile {
            app: "Finder".into(),
            title: String::new(),
            app_spans: vec![(0, 6)],
            ..Tile::default()
        };
        let (fallback, spans) = caption_parts(&empty_title, TitleShow::Title);
        assert_eq!(fallback, "Finder");
        assert_eq!(spans, vec![(0, 6)]);
        let (both_fallback, _) = caption_parts(&empty_title, TitleShow::Both);
        assert_eq!(both_fallback, "Finder");
    }

    #[test]
    fn auto_scale_is_monotone_nonincreasing() {
        for style in [Style::Gallery, Style::Icons, Style::List] {
            for &(sw, sh) in &[(1440.0, 900.0), (2560.0, 1440.0), (1280.0, 800.0)] {
                let mut prev = f64::MAX;
                for &n in &[1usize, 3, 8, 20, 60, 200] {
                    let s = auto_scale(style, n, sw, sh);
                    assert!(
                        s <= prev,
                        "scale rose with more tiles: {style:?} n={n} {s} > {prev} on {sw}x{sh}"
                    );
                    prev = s;
                }
            }
        }
    }

    #[test]
    fn auto_scale_panel_fits_screen() {
        for style in [Style::Gallery, Style::Icons, Style::List] {
            for &(sw, sh) in &[(1440.0, 900.0), (2560.0, 1440.0)] {
                for &n in &[1usize, 3, 8, 20, 60, 200] {
                    let s = auto_scale(style, n, sw, sh);
                    assert!(
                        panel_fits(style, n, &[], sw, sh, s, 0.0),
                        "auto_scale({style:?}, {n}, {sw}, {sh}) = {s} does not fit"
                    );
                    let floor = match style {
                        Style::List => 0.05,
                        Style::Gallery | Style::Icons => SCALE_FLOOR,
                    };
                    assert!(s >= floor);
                    assert!(s <= SCALE_LARGE);
                }
            }
        }
    }

    #[test]
    fn list_layout_is_a_single_column() {
        let plan = list_layout(3, 280.0, 30.0, 4.0, 12.0, 0.0, 900.0);
        assert_eq!(plan.width, 280.0 + 24.0);
        assert_eq!(plan.origins.len(), 3);
        assert_eq!(plan.origins[0].0, plan.origins[1].0);
        assert_eq!(plan.origins[1].0, plan.origins[2].0);
        assert!(plan.origins[0].1 > plan.origins[1].1);
        assert!(plan.origins[1].1 > plan.origins[2].1);
        assert_eq!(plan.rows, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn gallery_rows_match_layout_groups() {
        let plan = layout(&[100.0, 100.0, 100.0], 220.0);
        assert_eq!(plan.rows, vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn hit_tile_finds_containing_frame() {
        let frames = [
            NSRect::new(NSPoint::new(10.0, 10.0), NSSize::new(50.0, 40.0)),
            NSRect::new(NSPoint::new(70.0, 10.0), NSSize::new(50.0, 40.0)),
        ];
        assert_eq!(hit_tile(&frames, NSPoint::new(30.0, 20.0)), Some(0));
        assert_eq!(hit_tile(&frames, NSPoint::new(90.0, 20.0)), Some(1));
        assert_eq!(hit_tile(&frames, NSPoint::new(5.0, 5.0)), None);
    }

    #[test]
    fn scroll_steps_quantise_precise_deltas() {
        let mut accum = 0.0;
        assert!(scroll_steps(&mut accum, 10.0, true).is_empty());
        assert!((accum - 10.0).abs() < 1e-9);
        assert_eq!(scroll_steps(&mut accum, 35.0, true), vec![-1]);
        assert!((accum - 5.0).abs() < 1e-9);
        assert_eq!(scroll_steps(&mut accum, -85.0, true), vec![1, 1]);
        assert!((accum - 0.0).abs() < 1e-9);
    }

    #[test]
    fn scroll_steps_line_wheel_is_one_step() {
        let mut accum = 0.0;
        assert_eq!(scroll_steps(&mut accum, -1.0, false), vec![1]);
        assert!((accum - 0.0).abs() < 1e-9);
        assert_eq!(scroll_steps(&mut accum, 1.0, false), vec![-1]);
    }

    #[test]
    fn query_row_grows_panel_height() {
        let plain = layout(&[100.0], 1000.0);
        let with_q = layout_with_query(&[100.0], 1000.0, QUERY_H + GAP);
        assert_eq!(plain.width, with_q.width);
        assert_eq!(with_q.height, plain.height + QUERY_H + GAP);
        // Tiles stay bottom-anchored; the extra space is above them.
        assert_eq!(plain.origins[0].1, with_q.origins[0].1);
    }

    #[test]
    fn query_row_absent_keeps_legacy_height() {
        let plan = layout(&[100.0, 100.0], 1000.0);
        assert_eq!(plan.height, TILE_H + 2.0 * PAD);
        let empty = layout(&[], 1000.0);
        assert_eq!(empty.height, 2.0 * PAD);
        let empty_q = layout_with_query(&[], 1000.0, QUERY_H + GAP);
        assert_eq!(empty_q.height, 2.0 * PAD + QUERY_H + GAP);
    }

    #[test]
    fn utf8_span_converts_to_utf16_range() {
        // "Café" — é is one Unicode scalar, two UTF-8 bytes, one UTF-16 unit.
        let s = "Café";
        let range = utf8_span_to_utf16_range(s, 0, s.len()).unwrap();
        assert_eq!(range, NSRange::new(0, 4));
        let e_acute = utf8_span_to_utf16_range(s, 3, 2).unwrap();
        assert_eq!(e_acute, NSRange::new(3, 1));
        // Non-BMP: "𝄞" is U+1D11E — 4 UTF-8 bytes, 2 UTF-16 units (surrogate pair).
        let clef = "x𝄞y";
        let span = utf8_span_to_utf16_range(clef, 1, 4).unwrap();
        assert_eq!(span, NSRange::new(1, 2));
    }

    #[test]
    fn utf8_span_skips_invalid() {
        let s = "Café";
        assert!(utf8_span_to_utf16_range(s, 0, 0).is_none());
        assert!(utf8_span_to_utf16_range(s, 10, 1).is_none());
        assert!(utf8_span_to_utf16_range(s, 3, 5).is_none());
        // Mid-code-unit (inside é's UTF-8 sequence).
        assert!(utf8_span_to_utf16_range(s, 4, 1).is_none());
    }
}
