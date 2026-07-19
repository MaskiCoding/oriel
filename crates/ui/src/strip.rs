use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSPopUpMenuWindowLevel, NSTextField, NSView,
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

/// The resident overlay panel. Created once and kept alive; `show` updates the
/// tiles and orders it front, `hide` orders it out — never rebuilt.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    effect: Retained<NSVisualEffectView>,
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

        Self { mtm, panel, effect }
    }

    /// Replaces the tiles, sizes and centers the panel, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
        for sub in &self.effect.subviews() {
            sub.removeFromSuperview();
        }

        let n = f64::from(u32::try_from(tiles.len().max(1)).unwrap_or(u32::MAX));
        let width = 2.0f64.mul_add(PAD, n * TILE_W + (n - 1.0) * GAP);
        let height = 2.0f64.mul_add(PAD, TILE_H);
        self.panel.setContentSize(NSSize::new(width, height));

        for (i, tile) in tiles.iter().enumerate() {
            let x = (TILE_W + GAP).mul_add(f64::from(u32::try_from(i).unwrap_or(u32::MAX)), PAD);
            let view = self.tile(tile, i == selected);
            view.setFrame(NSRect::new(
                NSPoint::new(x, PAD),
                NSSize::new(TILE_W, TILE_H),
            ));
            self.effect.addSubview(&view);
        }

        self.panel.center();
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    fn tile(&self, tile: &Tile, selected: bool) -> Retained<NSView> {
        let text = NSString::from_str(&format!("{}\n{}", tile.app, tile.title));
        let label = NSTextField::labelWithString(&text, self.mtm);
        label.setMaximumNumberOfLines(2);
        label.setDrawsBackground(true);
        let bg = if selected {
            NSColor::controlAccentColor()
        } else {
            NSColor::clearColor()
        };
        label.setBackgroundColor(Some(&bg));
        label.setTextColor(Some(&NSColor::labelColor()));
        Retained::into_super(Retained::into_super(label))
    }
}
