use std::cell::{Cell, RefCell};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSImageView, NSPanel, NSPopUpMenuWindowLevel,
    NSRunningApplication, NSScreen, NSTextField, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCAGravityResizeAspect;

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
}

/// Inner margin between a tile's edge and its contents.
const INSET: f64 = 8.0;
/// Height of the preview box inside every tile.
const PREVIEW_H: f64 = 112.0;
/// Height of the caption row (app icon + title) above the preview.
const CAPTION_H: f64 = 22.0;
const TILE_H: f64 = INSET + PREVIEW_H + 4.0 + CAPTION_H + 4.0;
/// Bounds on a tile's width, whatever the window's shape.
const MIN_W: f64 = 96.0;
const MAX_W: f64 = 292.0;
const GAP: f64 = 14.0;
const PAD: f64 = 24.0;
/// The fallback app icon size when a tile has no preview.
const ICON: f64 = 56.0;

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// A tile's width follows its window's aspect at the shared preview height,
/// within limits that keep captions readable and panoramas polite.
fn tile_width(aspect: f64) -> f64 {
    (aspect * PREVIEW_H).clamp(MIN_W, MAX_W) + 2.0 * INSET
}

/// Where every tile goes: bottom-left origins in panel coordinates, rows
/// wrapped at the width limit and centered, plus the panel size.
struct Layout {
    origins: Vec<(f64, f64)>,
    width: f64,
    height: f64,
}

fn layout(widths: &[f64], max_content: f64) -> Layout {
    let mut rows: Vec<(Vec<usize>, f64)> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut row_w = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        if !row.is_empty() && row_w + GAP + w > max_content {
            rows.push((std::mem::take(&mut row), row_w));
            row_w = 0.0;
        }
        row_w += if row.is_empty() { w } else { GAP + w };
        row.push(i);
    }
    if !row.is_empty() {
        rows.push((row, row_w));
    }
    let content_w = rows.iter().map(|r| r.1).fold(0.0, f64::max);
    let count = as_f64(rows.len());
    let width = content_w + 2.0 * PAD;
    let height = count.mul_add(TILE_H, (count - 1.0).max(0.0) * GAP) + 2.0 * PAD;
    let mut origins = vec![(0.0, 0.0); widths.len()];
    for (r, (indices, w)) in rows.iter().enumerate() {
        let mut x = PAD + (content_w - w) / 2.0;
        let y = height - PAD - as_f64(r + 1) * TILE_H - as_f64(r) * GAP;
        for &i in indices {
            origins[i] = (x, y);
            x += widths[i] + GAP;
        }
    }
    Layout {
        origins,
        width,
        height,
    }
}

/// A translucent black backing for the badge chip, so it reads over any
/// preview.
fn scrim(view: &NSView) {
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        let color = NSColor::colorWithWhite_alpha(0.0, 0.5).CGColor();
        layer.setBackgroundColor(Some(&color));
    }
}

/// The selection: the whole tile lifts — a tinted rounded backing plus an
/// accent ring.
fn set_highlight(tile: &NSView, on: bool) {
    if let Some(layer) = tile.layer() {
        let border = on.then(|| NSColor::controlAccentColor().CGColor());
        layer.setBorderColor(border.as_deref());
        layer.setBorderWidth(if on { 2.5 } else { 0.0 });
        let backing = on.then(|| NSColor::colorWithWhite_alpha(1.0, 0.13).CGColor());
        layer.setBackgroundColor(backing.as_deref());
    }
}

fn app_icon(pid: i32) -> Option<Retained<objc2_app_kit::NSImage>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).and_then(|app| app.icon())
}

/// The resident overlay panel. Created once and kept alive: `show` builds the
/// tile grid and orders it front, `select` moves the highlight without
/// rebuilding, `hide` orders it out.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    content: Retained<NSView>,
    tiles: RefCell<Vec<Retained<NSView>>>,
    selected: Cell<usize>,
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

        let content = NSView::new(mtm);
        content.setWantsLayer(true);
        if let Some(layer) = content.layer() {
            layer.setCornerRadius(18.0);
            layer.setMasksToBounds(true);
            let backing = NSColor::colorWithWhite_alpha(0.12, 0.94).CGColor();
            layer.setBackgroundColor(Some(&backing));
        }
        panel.setContentView(Some(&content));

        Self {
            mtm,
            panel,
            content,
            tiles: RefCell::new(Vec::new()),
            selected: Cell::new(0),
        }
    }

    /// Builds the tiles in centered wrapping rows that always fit the screen,
    /// sizes and centers the panel, highlights `selected`, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
        for old in self.tiles.borrow().iter() {
            old.removeFromSuperview();
        }

        let widths: Vec<f64> = tiles.iter().map(|t| tile_width(t.aspect)).collect();
        let plan = layout(&widths, self.screen_width() * 0.9 - 2.0 * PAD);
        self.panel
            .setContentSize(NSSize::new(plan.width, plan.height));

        let mut views = Vec::with_capacity(tiles.len());
        for (i, tile) in tiles.iter().enumerate() {
            let view = self.tile(tile);
            let (x, y) = plan.origins[i];
            view.setFrameOrigin(NSPoint::new(x, y));
            self.content.addSubview(&view);
            views.push(view);
        }
        self.tiles.replace(views);
        self.selected.set(usize::MAX);
        self.select(selected);

        self.panel.center();
        self.panel.orderFrontRegardless();
    }

    /// Moves the highlight to `index` by restyling two tiles — the cheap path
    /// taken on every cycle, so it keeps pace with key-repeat.
    pub fn select(&self, index: usize) {
        let tiles = self.tiles.borrow();
        if index >= tiles.len() || index == self.selected.get() {
            return;
        }
        if let Some(old) = tiles.get(self.selected.get()) {
            set_highlight(old, false);
        }
        set_highlight(&tiles[index], true);
        self.selected.set(index);
    }

    /// Rebuilds one tile in place — the path a freshly captured preview takes
    /// while the strip is up. Keeps the frame and the selection highlight.
    pub fn update_tile(&self, index: usize, tile: &Tile) {
        let mut tiles = self.tiles.borrow_mut();
        let Some(slot) = tiles.get_mut(index) else {
            return;
        };
        let view = self.tile(tile);
        view.setFrameOrigin(slot.frame().origin);
        self.content.addSubview(&view);
        slot.removeFromSuperview();
        set_highlight(&view, index == self.selected.get());
        *slot = view;
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    fn screen_width(&self) -> f64 {
        NSScreen::mainScreen(self.mtm).map_or(1440.0, |s| s.frame().size.width)
    }

    /// A tile: caption row on top, preview (or large icon) below, marker chip
    /// over the preview's top-right corner.
    fn tile(&self, tile: &Tile) -> Retained<NSView> {
        let width = tile_width(tile.aspect);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, TILE_H));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(10.0);
            layer.setMasksToBounds(true);
        }

        self.caption(&view, tile, width);
        match &tile.preview {
            Some(image) => self.fill_preview(&view, image, width),
            None => self.fill_icon(&view, tile.pid, width),
        }
        if !tile.badge.is_empty() {
            self.add_badge(&view, &tile.badge, width);
        }
        view
    }

    /// The caption row above the preview: small app icon, then the window
    /// title (or app name), truncated to fit.
    fn caption(&self, view: &NSView, tile: &Tile, width: f64) {
        let base = TILE_H - 4.0 - CAPTION_H;
        let mut text_x = INSET;
        if let Some(icon) = app_icon(tile.pid) {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            image.setFrame(NSRect::new(
                NSPoint::new(INSET, base + 3.0),
                NSSize::new(16.0, 16.0),
            ));
            view.addSubview(&image);
            text_x += 22.0;
        }
        let text = if tile.title.is_empty() {
            &tile.app
        } else {
            &tile.title
        };
        let label = NSTextField::labelWithString(&NSString::from_str(text), self.mtm);
        label.setMaximumNumberOfLines(1);
        label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        label.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.9)));
        label.setFrame(NSRect::new(
            NSPoint::new(text_x, base + 2.0),
            NSSize::new(width - text_x - INSET, 17.0),
        ));
        view.addSubview(&label);
    }

    /// The whole window, aspect-fit inside the preview box.
    fn fill_preview(&self, view: &NSView, image: &CGImage, width: f64) {
        let host = NSView::initWithFrame(
            self.mtm.alloc(),
            NSRect::new(
                NSPoint::new(INSET, INSET),
                NSSize::new(width - 2.0 * INSET, PREVIEW_H),
            ),
        );
        host.setWantsLayer(true);
        if let Some(layer) = host.layer() {
            let contents: &AnyObject = unsafe { &*core::ptr::from_ref(image).cast::<AnyObject>() };
            unsafe { layer.setContents(Some(contents)) };
            layer.setContentsGravity(unsafe { kCAGravityResizeAspect });
            layer.setCornerRadius(6.0);
            layer.setMasksToBounds(true);
        }
        view.addSubview(&host);
    }

    /// Fallback: a large centered app icon in place of the preview.
    fn fill_icon(&self, view: &NSView, pid: i32, width: f64) {
        if let Some(icon) = app_icon(pid) {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            image.setFrame(NSRect::new(
                NSPoint::new((width - ICON) / 2.0, INSET + (PREVIEW_H - ICON) / 2.0),
                NSSize::new(ICON, ICON),
            ));
            view.addSubview(&image);
        }
    }

    /// The state-marker chip, a rounded scrim in the preview's top-right corner.
    fn add_badge(&self, view: &NSView, badge: &str, width: f64) {
        let label = NSTextField::labelWithString(&NSString::from_str(badge), self.mtm);
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        label.sizeToFit();
        let text = label.frame().size;
        let (w, h) = (text.width + 10.0, text.height + 4.0);
        let chip = NSView::initWithFrame(
            self.mtm.alloc(),
            NSRect::new(
                NSPoint::new(width - INSET - w - 6.0, INSET + PREVIEW_H - h - 6.0),
                NSSize::new(w, h),
            ),
        );
        scrim(&chip);
        if let Some(layer) = chip.layer() {
            layer.setCornerRadius(h / 2.0);
        }
        label.setFrameOrigin(NSPoint::new(5.0, 2.0));
        chip.addSubview(&label);
        view.addSubview(&chip);
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
    fn oversized_tile_still_gets_a_row() {
        let plan = layout(&[500.0, 100.0], 400.0);
        assert!(plan.origins[0].1 > plan.origins[1].1);
        assert_eq!(plan.width, 500.0 + 2.0 * PAD);
    }
}
