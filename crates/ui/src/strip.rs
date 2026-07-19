use std::cell::{Cell, RefCell};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// One entry to render in the strip.
pub struct Tile {
    pub app: String,
    pub title: String,
}

const TILE_W: f64 = 168.0;
const TILE_H: f64 = 84.0;
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

/// The resident overlay panel. Created once and kept alive: `show` builds the
/// tile grid and orders it front, `select` moves the highlight without
/// rebuilding, `hide` orders it out.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    effect: Retained<NSVisualEffectView>,
    labels: RefCell<Vec<Retained<NSTextField>>>,
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
            labels: RefCell::new(Vec::new()),
            selected: Cell::new(0),
        }
    }

    /// Builds the tiles in a wrapping grid that always fits the screen, sizes
    /// and centers the panel, highlights `selected`, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
        for old in self.labels.borrow().iter() {
            old.removeFromSuperview();
        }

        let count = tiles.len().max(1);
        let cols = columns(count, self.screen_width() * 0.92 - 2.0 * PAD);
        let rows = count.div_ceil(cols);

        let width = as_f64(cols).mul_add(TILE_W, as_f64(cols - 1) * GAP + 2.0 * PAD);
        let height = as_f64(rows).mul_add(TILE_H, as_f64(rows - 1) * GAP + 2.0 * PAD);
        self.panel.setContentSize(NSSize::new(width, height));

        let mut labels = Vec::with_capacity(tiles.len());
        for (i, tile) in tiles.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let x = (TILE_W + GAP).mul_add(as_f64(col), PAD);
            // NSView is bottom-left origin, so the top row sits at the largest y.
            let y = (TILE_H + GAP).mul_add(as_f64(rows - 1 - row), PAD);
            let label = self.label(tile);
            label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(TILE_W, TILE_H)));
            self.effect.addSubview(&label);
            labels.push(label);
        }
        self.labels.replace(labels);
        self.selected.set(usize::MAX);
        self.select(selected);

        self.panel.center();
        self.panel.orderFrontRegardless();
    }

    /// Moves the highlight to `index` by recoloring two tiles — the cheap path
    /// taken on every cycle, so it keeps pace with key-repeat.
    pub fn select(&self, index: usize) {
        let labels = self.labels.borrow();
        if index >= labels.len() || index == self.selected.get() {
            return;
        }
        if let Some(old) = labels.get(self.selected.get()) {
            old.setBackgroundColor(Some(&NSColor::clearColor()));
        }
        labels[index].setBackgroundColor(Some(&NSColor::controlAccentColor()));
        self.selected.set(index);
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    fn screen_width(&self) -> f64 {
        NSScreen::mainScreen(self.mtm).map_or(1440.0, |s| s.frame().size.width)
    }

    fn label(&self, tile: &Tile) -> Retained<NSTextField> {
        let text = NSString::from_str(&format!("{}\n{}", tile.app, tile.title));
        let label = NSTextField::labelWithString(&text, self.mtm);
        label.setMaximumNumberOfLines(2);
        label.setDrawsBackground(true);
        label.setBackgroundColor(Some(&NSColor::clearColor()));
        label.setTextColor(Some(&NSColor::labelColor()));
        label
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
