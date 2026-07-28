use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSImageView, NSPanel, NSPopUpMenuWindowLevel,
    NSRunningApplication, NSScreen, NSTextField, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSPointInRect, NSRect, NSSize, NSString};
use objc2_quartz_core::kCAGravityResizeAspect;

use crate::look::{Look, ShowOn, Size, Style, Theme};

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
const PREVIEW_H: f64 = 140.0;
/// Height of the caption row (app icon + title) above the preview.
const CAPTION_H: f64 = 22.0;
const TILE_H: f64 = INSET + PREVIEW_H + 4.0 + CAPTION_H + 4.0;
/// Bounds on a tile's width, whatever the window's shape.
const MIN_W: f64 = 110.0;
const MAX_W: f64 = 360.0;
const GAP: f64 = 14.0;
const PAD: f64 = 24.0;
/// The fallback app icon size when a tile has no preview.
const ICON: f64 = 64.0;
/// How full a row aims to be, as a share of the hard width limit — the strip
/// prefers growing down over stretching one row across the screen.
const ROW_FILL: f64 = 0.8;

/// Fixed scales for the discrete size settings; `Auto` picks in between.
const SCALE_SMALL: f64 = 0.7;
const SCALE_MEDIUM: f64 = 1.0;
const SCALE_LARGE: f64 = 1.4;
const SCALE_FLOOR: f64 = 0.2;

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Tile metrics at a given scale. Scale 1.0 matches the Medium Gallery constants.
#[derive(Clone, Copy)]
struct Metrics {
    preview_h: f64,
    caption_h: f64,
    icon: f64,
    min_w: f64,
    max_w: f64,
    gap: f64,
    pad: f64,
    inset: f64,
    title_font: f64,
    badge_font: f64,
    caption_icon: f64,
}

impl Metrics {
    fn at(scale: f64) -> Self {
        let s = scale;
        Self {
            preview_h: PREVIEW_H * s,
            caption_h: CAPTION_H * s,
            icon: ICON * s,
            min_w: MIN_W * s,
            max_w: MAX_W * s,
            gap: (GAP * s).max(6.0),
            pad: (PAD * s).max(10.0),
            inset: (INSET * s).max(3.0),
            title_font: (12.0 * s).clamp(10.0, 14.0),
            badge_font: (11.0 * s).clamp(9.0, 13.0),
            caption_icon: (16.0 * s).clamp(12.0, 20.0),
        }
    }

    fn gallery_tile_h(&self) -> f64 {
        self.inset + self.preview_h + 4.0 + self.caption_h + 4.0
    }

    fn icons_side(&self) -> f64 {
        self.icon + 6.0 + self.caption_h + 2.0 * self.inset
    }

    fn list_row_h(&self) -> f64 {
        self.caption_icon.max(self.caption_h) + 2.0 * self.inset
    }

    fn list_width(&self) -> f64 {
        (280.0 * self.preview_h / PREVIEW_H).clamp(160.0, 400.0)
    }
}

fn size_scale(size: Size, tiles: usize, screen_w: f64, screen_h: f64) -> f64 {
    match size {
        Size::Small => SCALE_SMALL,
        Size::Medium => SCALE_MEDIUM,
        Size::Large => SCALE_LARGE,
        Size::Auto => auto_scale(tiles, screen_w, screen_h),
    }
}

/// Picks a Gallery scale so every tile fits on one screen. More windows never
/// yield a larger scale; the floor keeps a huge session readable instead of
/// collapsing to zero.
fn auto_scale(tiles: usize, screen_w: f64, screen_h: f64) -> f64 {
    if tiles == 0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return SCALE_MEDIUM;
    }
    let candidates = [
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
    ];
    for &scale in &candidates {
        if panel_fits(tiles, screen_w, screen_h, scale) {
            return scale;
        }
    }
    SCALE_FLOOR
}

fn gallery_tile_width(aspect: f64, m: &Metrics) -> f64 {
    (aspect * m.preview_h).clamp(m.min_w, m.max_w) + 2.0 * m.inset
}

fn panel_fits(tiles: usize, screen_w: f64, screen_h: f64, scale: f64) -> bool {
    let m = Metrics::at(scale);
    let tw = gallery_tile_width(1.5, &m);
    let widths = vec![tw; tiles];
    let max_content = (screen_w * 0.9 - 2.0 * m.pad).max(m.min_w);
    let plan = layout_sized(&widths, max_content, m.gallery_tile_h(), m.gap, m.pad);
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
    width: f64,
    height: f64,
}

#[cfg(test)]
fn layout(widths: &[f64], max_content: f64) -> Layout {
    layout_sized(widths, max_content, TILE_H, GAP, PAD)
}

fn layout_sized(widths: &[f64], max_content: f64, tile_h: f64, gap: f64, pad: f64) -> Layout {
    if widths.is_empty() {
        return Layout {
            origins: Vec::new(),
            width: 2.0 * pad,
            height: 2.0 * pad,
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
    let height = count.mul_add(tile_h, (count - 1.0).max(0.0) * gap) + 2.0 * pad;
    let mut origins = vec![(0.0, 0.0); widths.len()];
    for (r, (indices, w)) in rows.iter().enumerate() {
        let mut x = pad + (content_w - w) / 2.0;
        let y = height - pad - as_f64(r + 1) * tile_h - as_f64(r) * gap;
        for &i in indices {
            origins[i] = (x, y);
            x += widths[i] + gap;
        }
    }
    Layout {
        origins,
        width,
        height,
    }
}

fn plan(tiles: &[Tile], style: Style, m: &Metrics, screen_w: f64) -> Layout {
    match style {
        Style::Gallery => {
            let widths: Vec<f64> = tiles
                .iter()
                .map(|t| gallery_tile_width(t.aspect, m))
                .collect();
            let max_content = (screen_w * 0.9 - 2.0 * m.pad).max(m.min_w);
            layout_sized(&widths, max_content, m.gallery_tile_h(), m.gap, m.pad)
        }
        Style::Icons => {
            let side = m.icons_side();
            let widths = vec![side; tiles.len()];
            let max_content = (screen_w * 0.9 - 2.0 * m.pad).max(side);
            layout_sized(&widths, max_content, side, m.gap, m.pad)
        }
        Style::List => list_layout(tiles.len(), m.list_width(), m.list_row_h(), m.gap, m.pad),
    }
}

fn list_layout(n: usize, row_w: f64, row_h: f64, gap: f64, pad: f64) -> Layout {
    let count = as_f64(n);
    let width = row_w + 2.0 * pad;
    let height = if n == 0 {
        2.0 * pad
    } else {
        count.mul_add(row_h, (count - 1.0) * gap) + 2.0 * pad
    };
    let mut origins = Vec::with_capacity(n);
    for i in 0..n {
        let y = height - pad - as_f64(i + 1) * row_h - as_f64(i) * gap;
        origins.push((pad, y));
    }
    Layout {
        origins,
        width,
        height,
    }
}

/// The selection: the whole tile lifts — a tinted rounded backing plus an
/// accent ring.
fn set_highlight(tile: &NSView, on: bool, dark: bool) {
    if let Some(layer) = tile.layer() {
        let border = on.then(|| NSColor::controlAccentColor().CGColor());
        layer.setBorderColor(border.as_deref());
        layer.setBorderWidth(if on { 2.5 } else { 0.0 });
        let backing = on.then(|| {
            if dark {
                NSColor::colorWithWhite_alpha(1.0, 0.13).CGColor()
            } else {
                NSColor::colorWithWhite_alpha(0.0, 0.08).CGColor()
            }
        });
        layer.setBackgroundColor(backing.as_deref());
    }
}

fn app_icon(pid: i32) -> Option<Retained<objc2_app_kit::NSImage>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).and_then(|app| app.icon())
}

fn mouse_location() -> NSPoint {
    // SAFETY: `+[NSEvent mouseLocation]` returns an `NSPoint` by value.
    unsafe { msg_send![class!(NSEvent), mouseLocation] }
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

/// The resident overlay panel. Created once and kept alive: `show` builds the
/// tile grid and orders it front, `select` moves the highlight without
/// rebuilding, `hide` orders it out.
pub struct Strip {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    content: Retained<NSView>,
    tiles: RefCell<Vec<Retained<NSView>>>,
    selected: Cell<usize>,
    look: Cell<Look>,
    dark: Cell<bool>,
    /// Scale last applied by `show`, so `update_tile` rebuilds at the same size.
    scale: Cell<f64>,
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
            look: Cell::new(Look::default()),
            dark: Cell::new(true),
            scale: Cell::new(SCALE_MEDIUM),
        }
    }

    /// Sets the presentation used by the next `show`.
    pub fn set_look(&self, look: Look) {
        self.look.set(look);
    }

    /// Builds the tiles in centered wrapping rows that always fit the screen,
    /// sizes and centers the panel, highlights `selected`, and orders it front
    /// without activating Oriel.
    pub fn show(&self, tiles: &[Tile], selected: usize) {
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

        let scale = size_scale(look.size, tiles.len(), screen_w, screen_h);
        self.scale.set(scale);
        let m = Metrics::at(scale);
        let dark = theme_is_dark(look.theme);
        self.dark.set(dark);
        self.apply_theme(dark);

        let plan = plan(tiles, look.style, &m, screen_w);
        self.panel
            .setContentSize(NSSize::new(plan.width, plan.height));

        let mut views = Vec::with_capacity(tiles.len());
        for (i, tile) in tiles.iter().enumerate() {
            let view = self.tile(tile, look.style, &m, dark);
            let (x, y) = plan.origins[i];
            view.setFrameOrigin(NSPoint::new(x, y));
            self.content.addSubview(&view);
            views.push(view);
        }
        self.tiles.replace(views);
        self.selected.set(usize::MAX);
        self.select(selected);

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

    /// Moves the highlight to `index` by restyling two tiles — the cheap path
    /// taken on every cycle, so it keeps pace with key-repeat.
    pub fn select(&self, index: usize) {
        let tiles = self.tiles.borrow();
        if index >= tiles.len() || index == self.selected.get() {
            return;
        }
        let dark = self.dark.get();
        if let Some(old) = tiles.get(self.selected.get()) {
            set_highlight(old, false, dark);
        }
        set_highlight(&tiles[index], true, dark);
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
        self.content.addSubview(&view);
        slot.removeFromSuperview();
        set_highlight(&view, index == self.selected.get(), dark);
        *slot = view;
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    fn apply_theme(&self, dark: bool) {
        if let Some(layer) = self.content.layer() {
            let backing = if dark {
                NSColor::colorWithWhite_alpha(0.12, 0.94).CGColor()
            } else {
                NSColor::colorWithWhite_alpha(0.96, 0.94).CGColor()
            };
            layer.setBackgroundColor(Some(&backing));
        }
    }

    fn target_screen(&self, show_on: ShowOn) -> Option<Retained<NSScreen>> {
        match show_on {
            ShowOn::ActiveScreen => NSScreen::mainScreen(self.mtm),
            ShowOn::MenubarScreen => NSScreen::screens(self.mtm)
                .firstObject()
                .or_else(|| NSScreen::mainScreen(self.mtm)),
            ShowOn::PointerScreen => {
                let loc = mouse_location();
                let screens = NSScreen::screens(self.mtm);
                let n = screens.count();
                for i in 0..n {
                    let screen = screens.objectAtIndex(i);
                    if NSPointInRect(loc, screen.frame()) {
                        return Some(screen);
                    }
                }
                NSScreen::mainScreen(self.mtm)
            }
        }
    }

    fn tile(&self, tile: &Tile, style: Style, m: &Metrics, dark: bool) -> Retained<NSView> {
        match style {
            Style::Gallery => self.gallery_tile(tile, m, dark),
            Style::Icons => self.icons_tile(tile, m, dark),
            Style::List => self.list_tile(tile, m, dark),
        }
    }

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

    /// Icons: large app icon with the title beneath; uniform square-ish tile.
    fn icons_tile(&self, tile: &Tile, m: &Metrics, dark: bool) -> Retained<NSView> {
        let side = m.icons_side();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side, side));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(10.0);
            layer.setMasksToBounds(true);
        }

        if let Some(icon) = app_icon(tile.pid) {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            let icon_y = m.inset + m.caption_h + 6.0;
            image.setFrame(NSRect::new(
                NSPoint::new((side - m.icon) / 2.0, icon_y),
                NSSize::new(m.icon, m.icon),
            ));
            view.addSubview(&image);
        }

        self.caption_row(&view, tile, side, m.inset, m.caption_h, m, dark, false);
        view
    }

    /// List: dense row — small icon, title, markers right-aligned.
    fn list_tile(&self, tile: &Tile, m: &Metrics, dark: bool) -> Retained<NSView> {
        let width = m.list_width();
        let height = m.list_row_h();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let view = NSView::initWithFrame(self.mtm.alloc(), frame);
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(6.0);
            layer.setMasksToBounds(true);
        }

        let band = height - 2.0 * m.inset;
        self.caption_row(&view, tile, width, m.inset, band, m, dark, true);
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
        if with_icon && let Some(icon) = app_icon(tile.pid) {
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
        if !tile.badge.is_empty() {
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

        let text = if tile.title.is_empty() {
            &tile.app
        } else {
            &tile.title
        };
        let label = NSTextField::labelWithString(&NSString::from_str(text), self.mtm);
        label.setMaximumNumberOfLines(1);
        label.setFont(Some(&NSFont::systemFontOfSize(m.title_font)));
        label.setTextColor(Some(&title_color));
        let label_h = m.title_font + 5.0;
        let label_y = base + (band_h - label_h).max(0.0) / 2.0;
        label.setFrame(NSRect::new(
            NSPoint::new(text_x, label_y),
            NSSize::new((text_end - text_x).max(10.0), label_h),
        ));
        view.addSubview(&label);
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
        if let Some(icon) = app_icon(pid) {
            let image = NSImageView::new(self.mtm);
            image.setImage(Some(&icon));
            image.setFrame(NSRect::new(
                NSPoint::new(
                    (width - 2.0 * m.inset - m.icon) / 2.0,
                    (m.preview_h - m.icon) / 2.0,
                ),
                NSSize::new(m.icon, m.icon),
            ));
            host.addSubview(&image);
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
    }

    #[test]
    fn medium_metrics_match_legacy_constants() {
        let m = Metrics::at(SCALE_MEDIUM);
        assert_eq!(m.preview_h, PREVIEW_H);
        assert_eq!(m.caption_h, CAPTION_H);
        assert_eq!(m.icon, ICON);
        assert_eq!(m.min_w, MIN_W);
        assert_eq!(m.max_w, MAX_W);
        assert_eq!(m.gap, GAP);
        assert_eq!(m.pad, PAD);
        assert_eq!(m.inset, INSET);
        assert_eq!(m.gallery_tile_h(), TILE_H);
    }

    #[test]
    fn auto_scale_is_monotone_nonincreasing() {
        for &(sw, sh) in &[(1440.0, 900.0), (2560.0, 1440.0), (1280.0, 800.0)] {
            let mut prev = f64::MAX;
            for &n in &[1usize, 3, 8, 20, 60, 200] {
                let s = auto_scale(n, sw, sh);
                assert!(
                    s <= prev,
                    "scale rose with more tiles: n={n} {s} > {prev} on {sw}x{sh}"
                );
                prev = s;
            }
        }
    }

    #[test]
    fn auto_scale_panel_fits_screen() {
        for &(sw, sh) in &[(1440.0, 900.0), (2560.0, 1440.0)] {
            for &n in &[1usize, 3, 8, 20, 60, 200] {
                let s = auto_scale(n, sw, sh);
                assert!(
                    panel_fits(n, sw, sh, s),
                    "auto_scale({n}, {sw}, {sh}) = {s} does not fit"
                );
                assert!(s >= SCALE_FLOOR);
                assert!(s <= SCALE_LARGE);
            }
        }
    }

    #[test]
    fn list_layout_is_a_single_column() {
        let plan = list_layout(3, 280.0, 30.0, 4.0, 12.0);
        assert_eq!(plan.width, 280.0 + 24.0);
        assert_eq!(plan.origins.len(), 3);
        assert_eq!(plan.origins[0].0, plan.origins[1].0);
        assert_eq!(plan.origins[1].0, plan.origins[2].0);
        assert!(plan.origins[0].1 > plan.origins[1].1);
        assert!(plan.origins[1].1 > plan.origins[2].1);
    }
}
