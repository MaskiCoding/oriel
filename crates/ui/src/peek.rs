use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSScreen, NSStatusWindowLevel, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::kCAGravityResizeAspect;

/// Cap on how much of the screen's visible frame Peek may occupy.
const SCREEN_FILL: f64 = 0.8;
/// Matches the strip's content corner radius.
const CORNER: f64 = 18.0;

/// Aspect-fits `(img_w, img_h)` into `(max_w, max_h)` without upscaling past 1:1.
fn fit_size(img_w: f64, img_h: f64, max_w: f64, max_h: f64) -> (f64, f64) {
    if img_w <= 0.0 || img_h <= 0.0 || max_w <= 0.0 || max_h <= 0.0 {
        return (0.0, 0.0);
    }
    let scale = (max_w / img_w).min(max_h / img_h).min(1.0);
    (img_w * scale, img_h * scale)
}

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Full-size preview of the selection, ordered behind the strip. Resident
/// panel: created once, `show` updates contents, `hide` orders out.
pub struct Peek {
    panel: Retained<NSPanel>,
    content: Retained<NSView>,
}

impl Peek {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 200.0)),
            NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        // Below the strip's `NSPopUpMenuWindowLevel` (101), above ordinary windows.
        panel.setLevel(NSStatusWindowLevel);
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
            layer.setCornerRadius(CORNER);
            layer.setMasksToBounds(true);
            let backing = NSColor::colorWithWhite_alpha(0.12, 0.94).CGColor();
            layer.setBackgroundColor(Some(&backing));
        }
        panel.setContentView(Some(&content));

        Self { panel, content }
    }

    /// Shows `image` centred on `screen`, sized to fit without upscaling past
    /// the source resolution. Ordered behind the strip.
    pub fn show(&self, image: &CGImage, screen: &NSScreen) {
        let scale = screen.backingScaleFactor().max(1.0);
        let px_w = as_f64(CGImage::width(Some(image)));
        let px_h = as_f64(CGImage::height(Some(image)));
        // Points at 1:1 pixel mapping on this screen — never stretch past that.
        let native_w = px_w / scale;
        let native_h = px_h / scale;

        let vf = screen.visibleFrame();
        let max_w = vf.size.width * SCREEN_FILL;
        let max_h = vf.size.height * SCREEN_FILL;
        let (w, h) = fit_size(native_w, native_h, max_w, max_h);
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        self.panel.setContentSize(NSSize::new(w, h));
        if let Some(layer) = self.content.layer() {
            let contents: &AnyObject = unsafe { &*core::ptr::from_ref(image).cast::<AnyObject>() };
            unsafe { layer.setContents(Some(contents)) };
            layer.setContentsGravity(unsafe { kCAGravityResizeAspect });
        }

        self.panel.setFrameOrigin(NSPoint::new(
            (vf.size.width - w).mul_add(0.5, vf.origin.x),
            (vf.size.height - h).mul_add(0.5, vf.origin.y),
        ));
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn fit_shrinks_to_box() {
        let (w, h) = fit_size(2000.0, 1000.0, 800.0, 600.0);
        assert_eq!(w, 800.0);
        assert_eq!(h, 400.0);
    }

    #[test]
    fn fit_never_upscales() {
        let (w, h) = fit_size(400.0, 300.0, 2000.0, 1500.0);
        assert_eq!(w, 400.0);
        assert_eq!(h, 300.0);
    }

    #[test]
    fn fit_respects_height_limit() {
        let (w, h) = fit_size(1600.0, 1200.0, 1000.0, 600.0);
        assert_eq!(h, 600.0);
        assert_eq!(w, 800.0);
    }

    #[test]
    fn fit_zero_input_is_zero() {
        assert_eq!(fit_size(0.0, 100.0, 800.0, 600.0), (0.0, 0.0));
        assert_eq!(fit_size(100.0, 100.0, 0.0, 600.0), (0.0, 0.0));
    }
}
