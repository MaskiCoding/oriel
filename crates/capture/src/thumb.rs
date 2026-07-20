//! Downscales captures to tile size, so the cache holds kilobytes per window
//! instead of full-resolution frames.

use core::ffi::c_void;

use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, CGInterpolationQuality,
};

/// Twice the largest tile's preview box, so previews stay sharp on Retina
/// panels. Fit, not cover: tiles show the whole window, letterboxed.
const MAX_W: usize = 720;
const MAX_H: usize = 280;

/// What a cached image costs against the cache's byte budget.
pub fn cost(image: &CGImage) -> usize {
    CGImage::width(Some(image)) * CGImage::height(Some(image)) * 4
}

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `image` scaled down, aspect preserved, to fit inside a Retina tile's
/// preview box. Returns the original when it is already that small, or when
/// scaling fails.
pub fn thumbnail(image: CFRetained<CGImage>) -> CFRetained<CGImage> {
    let (w, h) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    if w <= MAX_W && h <= MAX_H {
        return image;
    }
    // Integer fit-scale: the binding axis lands exactly on the box edge.
    let (out_w, out_h) = if w * MAX_H >= h * MAX_W {
        (MAX_W, (h * MAX_W / w).max(1))
    } else {
        ((w * MAX_H / h).max(1), MAX_H)
    };
    let Some(space) = CGColorSpace::new_device_rgb() else {
        return image;
    };
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    let Some(ctx) = (unsafe {
        CGBitmapContextCreate(
            core::ptr::null_mut::<c_void>(),
            out_w,
            out_h,
            8,
            0,
            Some(&space),
            info,
        )
    }) else {
        return image;
    };
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::Medium);
    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(as_f64(out_w), as_f64(out_h)),
    );
    CGContext::draw_image(Some(&ctx), rect, Some(&image));
    CGBitmapContextCreateImage(Some(&ctx)).unwrap_or(image)
}
