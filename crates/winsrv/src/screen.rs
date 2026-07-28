/// Index of the frame containing `rect`'s centre, else the largest overlap, else 0.
///
/// Tuples are `(x, y, w, h)` in a shared top-left coordinate space.
pub fn screen_index(rect: (f64, f64, f64, f64), frames: &[(f64, f64, f64, f64)]) -> u32 {
    if frames.is_empty() {
        return 0;
    }
    let (x, y, w, h) = rect;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    for (i, &(fx, fy, fw, fh)) in frames.iter().enumerate() {
        if cx >= fx && cx < fx + fw && cy >= fy && cy < fy + fh {
            return u32::try_from(i).unwrap_or(0);
        }
    }
    let mut best_i = 0_u32;
    let mut best_area = -1.0_f64;
    for (i, &frame) in frames.iter().enumerate() {
        let area = overlap_area(rect, frame);
        if area > best_area {
            best_area = area;
            best_i = u32::try_from(i).unwrap_or(0);
        }
    }
    if best_area > 0.0 { best_i } else { 0 }
}

fn overlap_area(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    let x0 = ax.max(bx);
    let y0 = ay.max(by);
    let x1 = (ax + aw).min(bx + bw);
    let y1 = (ay + ah).min(by + bh);
    (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_containment_picks_that_display() {
        let frames = [(0.0, 0.0, 100.0, 100.0), (100.0, 0.0, 100.0, 100.0)];
        assert_eq!(screen_index((10.0, 10.0, 20.0, 20.0), &frames), 0);
        assert_eq!(screen_index((120.0, 10.0, 20.0, 20.0), &frames), 1);
    }

    #[test]
    fn straddling_window_uses_centre_then_overlap() {
        let frames = [(0.0, 0.0, 100.0, 100.0), (100.0, 0.0, 100.0, 100.0)];
        // Centre at x=95 → still on the left display.
        assert_eq!(screen_index((70.0, 40.0, 50.0, 20.0), &frames), 0);
        // Centre at x=105 → right display.
        assert_eq!(screen_index((80.0, 40.0, 50.0, 20.0), &frames), 1);
        // Fully between displays vertically off both centres — larger overlap wins.
        let gap_frames = [(0.0, 0.0, 100.0, 100.0), (200.0, 0.0, 100.0, 100.0)];
        assert_eq!(screen_index((50.0, 0.0, 120.0, 50.0), &gap_frames), 0);
        assert_eq!(screen_index((130.0, 0.0, 120.0, 50.0), &gap_frames), 1);
    }

    #[test]
    fn fully_off_any_display_falls_back_to_zero() {
        let frames = [(0.0, 0.0, 100.0, 100.0), (100.0, 0.0, 100.0, 100.0)];
        assert_eq!(screen_index((-200.0, -200.0, 10.0, 10.0), &frames), 0);
    }

    #[test]
    fn empty_frame_list_is_zero() {
        assert_eq!(screen_index((0.0, 0.0, 10.0, 10.0), &[]), 0);
    }

    #[test]
    fn zero_sized_window_uses_its_origin_as_centre() {
        let frames = [(0.0, 0.0, 100.0, 100.0), (100.0, 0.0, 100.0, 100.0)];
        assert_eq!(screen_index((150.0, 50.0, 0.0, 0.0), &frames), 1);
        assert_eq!(screen_index((50.0, 50.0, 0.0, 0.0), &frames), 0);
    }
}
