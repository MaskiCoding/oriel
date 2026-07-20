use std::cell::{Cell, RefCell};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSImageView, NSPanel, NSPopUpMenuWindowLevel,
    NSRunningApplication, NSScreen, NSTextAlignment, NSTextField, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCAGravityResizeAspectFill;

/// One entry to render in the strip.
pub struct Tile {
    pub app: String,
    pub title: String,
    pub pid: i32,
    /// A live window screenshot, when previews are available (Gallery style).
    /// `None` falls back to the app-icon layout (Icons style / no permission).
    pub preview: Option<CFRetained<CGImage>>,
    /// State markers (minimized ●, fullscreen ⤢, Desktop number), already
    /// composed; empty shows no chip.
    pub badge: String,
}

const TILE_W: f64 = 168.0;
const TILE_H: f64 = 96.0;
const ICON: f64 = 40.0;
/// Height of the scrimmed caption bar on a Gallery tile.
const CAPTION_H: f64 = 26.0;
const GAP: f64 = 12.0;
const PAD: f64 = 16.0;
/// Cap on tiles per row before wrapping, matching the PRD's ~7/row target.
const MAX_COLS: usize = 7;

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Tiles per row: as many as fit in `max_content` width, capped at `MAX_COLS`,
/// never more than the tile count. The rest wrap onto further rows.
fn columns(count: usize, max_content: f64) -> usize {
    let mut cols = 1;
    while cols < count && cols < MAX_COLS {
        let width = as_f64(cols + 1).mul_add(TILE_W, as_f64(cols) * GAP);
        if width > max_content {
            break;
        }
        cols += 1;
    }
    cols
}

/// The selection ring — an accent border, so it reads over a preview instead of
/// being hidden behind it.
fn set_highlight(tile: &NSView, on: bool) {
    if let Some(layer) = tile.layer() {
        let color = on.then(|| NSColor::controlAccentColor().CGColor());
        layer.setBorderColor(color.as_deref());
        layer.setBorderWidth(if on { 3.0 } else { 0.0 });
    }
}

/// The resident overlay panel. Created once and kept alive: `show` builds the
/// tile grid and orders it front, `select` moves the highlight without
/// rebuilding, `hide` orders it out.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    effect: Retained<NSVisualEffectView>,
    tiles: RefCell<Vec<Retained<NSView>>>,
    selected: Cell<usize>,
}

impl Strip {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(TILE_W, TILE_H)),
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

        let effect = NSVisualEffectView::new(mtm);
        effect.setMaterial(NSVisualEffectMaterial::HUDWindow);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setState(NSVisualEffectState::Active);
        panel.setContentView(Some(&effect));

        Self {
            mtm,
            panel,
            effect,
            tiles: RefCell::new(Vec::new()),
            selected: Cell::new(0),
        }
    }

    /// Builds the tiles in a wrapping grid that always fits the screen, sizes
    /// and centers the panel, highlights `selected`, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
        for old in self.tiles.borrow().iter() {
            old.removeFromSuperview();
        }

        let count = tiles.len().max(1);
        let cols = columns(count, self.screen_width() * 0.92 - 2.0 * PAD);
        let rows = count.div_ceil(cols);

        let width = as_f64(cols).mul_add(TILE_W, as_f64(cols - 1) * GAP + 2.0 * PAD);
        let height = as_f64(rows).mul_add(TILE_H, as_f64(rows - 1) * GAP + 2.0 * PAD);
        self.panel.setContentSize(NSSize::new(width, height));

        let mut views = Vec::with_capacity(tiles.len());
        for (i, tile) in tiles.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let x = (TILE_W + GAP).mul_add(as_f64(col), PAD);
            // NSView is bottom-left origin, so the top row sits at the largest y.
            let y = (TILE_H + GAP).mul_add(as_f64(rows - 1 - row), PAD);
            let view = self.tile(tile);
            view.setFrameOrigin(NSPoint::new(x, y));
            self.effect.addSubview(&view);
            views.push(view);
        }
        self.tiles.replace(views);
        self.selected.set(usize::MAX);
        self.select(selected);

        self.panel.center();
        self.panel.orderFrontRegardless();
    }

    /// Moves the highlight to `index` by recoloring two tiles — the cheap path
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
        self.effect.addSubview(&view);
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

    /// A tile, layer-backed and rounded so the preview clips and the selection
    /// border sits inside it. Gallery style when a preview is present, otherwise
    /// the app-icon layout.
    fn tile(&self, tile: &Tile) -> Retained<NSView> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(TILE_W, TILE_H));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(10.0);
            layer.setMasksToBounds(true);
        }

        match &tile.preview {
            Some(image) => self.fill_preview(&view, tile, image),
            None => self.fill_icon(&view, tile),
        }
        if !tile.badge.is_empty() {
            self.add_badge(&view, &tile.badge);
        }
        view
    }

    /// The state-marker chip, a rounded scrim in the tile's top-right corner.
    fn add_badge(&self, view: &NSView, badge: &str) {
        let label = NSTextField::labelWithString(&NSString::from_str(badge), self.mtm);
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        label.sizeToFit();
        let text = label.frame().size;
        let (w, h) = (text.width + 10.0, text.height + 4.0);
        let chip = NSView::initWithFrame(
            self.mtm.alloc(),
            NSRect::new(
                NSPoint::new(TILE_W - w - 6.0, TILE_H - h - 6.0),
                NSSize::new(w, h),
            ),
        );
        chip.setWantsLayer(true);
        if let Some(layer) = chip.layer() {
            let scrim = NSColor::colorWithWhite_alpha(0.0, 0.5).CGColor();
            layer.setBackgroundColor(Some(&scrim));
            layer.setCornerRadius(h / 2.0);
        }
        label.setFrameOrigin(NSPoint::new(5.0, 2.0));
        chip.addSubview(&label);
        view.addSubview(&chip);
    }

    /// Gallery tile: the screenshot fills the tile, with a scrimmed caption bar
    /// (app icon + title) across the bottom for legibility over any image.
    fn fill_preview(&self, view: &NSView, tile: &Tile, image: &CGImage) {
        if let Some(layer) = view.layer() {
            let contents: &AnyObject = unsafe { &*core::ptr::from_ref(image).cast::<AnyObject>() };
            unsafe { layer.setContents(Some(contents)) };
            layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        }

        let bar = NSView::initWithFrame(
            self.mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(TILE_W, CAPTION_H)),
        );
        bar.setWantsLayer(true);
        if let Some(layer) = bar.layer() {
            let scrim = NSColor::colorWithWhite_alpha(0.0, 0.5).CGColor();
            layer.setBackgroundColor(Some(&scrim));
        }

        if let Some(icon) = NSRunningApplication::runningApplicationWithProcessIdentifier(tile.pid)
            .and_then(|app| app.icon())
        {
            let view = NSImageView::new(self.mtm);
            view.setImage(Some(&icon));
            view.setFrame(NSRect::new(NSPoint::new(6.0, 4.0), NSSize::new(18.0, 18.0)));
            bar.addSubview(&view);
        }

        let text = if tile.title.is_empty() {
            &tile.app
        } else {
            &tile.title
        };
        let label = NSTextField::labelWithString(&NSString::from_str(text), self.mtm);
        label.setMaximumNumberOfLines(1);
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFrame(NSRect::new(
            NSPoint::new(28.0, 3.0),
            NSSize::new(TILE_W - 34.0, 20.0),
        ));
        bar.addSubview(&label);

        view.addSubview(&bar);
    }

    /// Fallback tile: app icon over app name + window title, centered.
    fn fill_icon(&self, view: &NSView, tile: &Tile) {
        if let Some(icon) = NSRunningApplication::runningApplicationWithProcessIdentifier(tile.pid)
            .and_then(|app| app.icon())
        {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            image.setFrame(NSRect::new(
                NSPoint::new((TILE_W - ICON) / 2.0, TILE_H - ICON - 8.0),
                NSSize::new(ICON, ICON),
            ));
            view.addSubview(&image);
        }

        let text = NSString::from_str(&format!("{}\n{}", tile.app, tile.title));
        let label = NSTextField::labelWithString(&text, self.mtm);
        label.setMaximumNumberOfLines(2);
        label.setAlignment(NSTextAlignment::Center);
        label.setTextColor(Some(&NSColor::labelColor()));
        label.setFrame(NSRect::new(
            NSPoint::new(6.0, 6.0),
            NSSize::new(TILE_W - 12.0, TILE_H - ICON - 14.0),
        ));
        view.addSubview(&label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fit_within_width_and_cap() {
        // one tile → one column
        assert_eq!(columns(1, 10_000.0), 1);
        // plenty of width but capped at MAX_COLS
        assert_eq!(columns(20, 10_000.0), MAX_COLS);
        // never more columns than tiles
        assert_eq!(columns(3, 10_000.0), 3);
        // narrow width forces fewer columns
        assert_eq!(columns(10, TILE_W + TILE_W + GAP), 2);
        // width for less than one tile still yields one column
        assert_eq!(columns(5, 10.0), 1);
    }
}
