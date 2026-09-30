//! Pure desktop geometry. Pointer gestures resize around a fixed anchor;
//! clamping never relocates a window to a different corner.
use super::LRect;

pub fn stream_aspect(width: f64, height: f64) -> Option<f64> {
    let ratio = width / height;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0
        && ratio.is_finite() && ratio > 0.0).then_some(ratio)
}

/// The center identifies the quadrant containing most of this rectangle.
/// Coordinates are relative to the current monitor's usable desktop area.
pub fn quadrant_anchor(r: LRect, area: LRect) -> (f64, f64) {
    (if r.x + r.w / 2.0 < area.x + area.w / 2.0 { 0.0 } else { 1.0 },
     if r.y + r.h / 2.0 < area.y + area.h / 2.0 { 0.0 } else { 1.0 })
}

/// Limit growth at the fixed corner instead of moving it to fit the screen.
fn anchored_size(r: LRect, width: f64, aspect: f64, area: LRect, anchor: (f64, f64), minimum: f64) -> LRect {
    let m = 16.0_f64.min(area.w.min(area.h) * 0.05).max(0.0);
    let (x, y) = (r.x + r.w * anchor.0, r.y + r.h * anchor.1);
    let room = |point: f64, start: f64, end: f64, a: f64| {
        let before = if a > 0.0 { (point - start) / a } else { f64::INFINITY };
        let after = if a < 1.0 { (end - point) / (1.0 - a) } else { f64::INFINITY };
        before.min(after).max(1.0)
    };
    let max_w = room(x, area.x + m, area.x + area.w - m, anchor.0)
        .min(room(y, area.y + m, area.y + area.h - m, anchor.1) * aspect).max(1.0);
    let w = width.clamp(minimum.min(max_w), max_w);
    let h = w / aspect;
    LRect { x: x - w * anchor.0, y: y - h * anchor.1, w, h }
}

/// Keep the quadrant's corner when metadata changes the stream shape.
pub fn with_aspect(r: LRect, aspect: f64, area: LRect) -> LRect {
    anchored_size(r, r.w, aspect, area, quadrant_anchor(r, area), 1.0)
}

/// Native resize proposals may change either dimension. Honor the dimension
/// that moved most, then restore the stream ratio before the next gesture.
pub fn native_resize(previous: LRect, w: f64, h: f64, aspect: f64, area: LRect) -> LRect {
    let w = if (w - previous.w).abs() >= (h - previous.h).abs() * aspect { w } else { h * aspect };
    anchored_size(previous, w, aspect, area, quadrant_anchor(previous, area), 1.0)
}

pub fn fit(mut r: LRect, area: LRect, margin: f64) -> LRect {
    let m = margin.min(area.w.min(area.h) * 0.05).max(0.0);
    let (w, h) = ((area.w - m * 2.0).max(1.0), (area.h - m * 2.0).max(1.0));
    let factor = (w / r.w.max(1.0)).min(h / r.h.max(1.0)).min(1.0);
    r.w = (r.w * factor).max(1.0);
    r.h = (r.h * factor).max(1.0);
    r.x = r.x.clamp(area.x + m, (area.x + area.w - m - r.w).max(area.x+m));
    r.y = r.y.clamp(area.y + m, (area.y + area.h - m - r.h).max(area.y+m));
    r
}

pub fn zoom(r: LRect, factor: f64, area: LRect, anchor: (f64, f64)) -> LRect {
    if !factor.is_finite() || factor <= 0.0 { return r; }
    let aspect = r.w / r.h.max(1.0);
    anchored_size(r, r.w * factor, aspect, area, anchor, 240.0_f64.min(240.0 * aspect))
}

/// Drag direction controls size; the quadrant corner stays fixed.
pub fn resize(r: LRect, delta: (f64, f64), edge: (i8, i8), area: LRect) -> LRect {
    let dx = delta.0 * edge.0 as f64;
    let dy = delta.1 * edge.1 as f64 * r.w / r.h.max(1.0);
    let change = if edge.0 == 0 {dy} else if edge.1 == 0 || dx.abs() >= dy.abs() {dx} else {dy};
    let anchor = quadrant_anchor(r, area);
    zoom(r, (r.w + change).max(1.0) / r.w.max(1.0), area, anchor)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn area() -> LRect { LRect{x:-1600.0,y:24.0,w:1600.0,h:916.0} }
    #[test]
    fn stream_shape_survives_source_changes_native_resize_and_gestures() {
        let mut r=LRect{x:-1000.0,y:300.0,w:480.0,h:270.0};
        for aspect in [9.0/16.0,4.0/3.0,21.0/9.0,1.0,16.0/9.0] {
            r=with_aspect(r,aspect,area());
            for edge in [(-1,-1),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                let sized=resize(r,(35.0,20.0),edge,area());
                assert!((sized.w/sized.h-aspect).abs()<1e-10);
            }
            for (w,h) in [(600.0,300.0),(250.0,700.0),(4000.0,3000.0)] {
                let sized=native_resize(r,w,h,aspect,area());
                assert!((sized.w/sized.h-aspect).abs()<1e-10);
                assert!(sized.x+sized.w<=area().x+area().w && sized.y+sized.h<=area().y+area().h);
            }
            r=zoom(r,1.2,area(),(0.5,0.5));
            assert!((r.w/r.h-aspect).abs()<1e-10);
        }
    }
    #[test]
    fn quadrant_corners_hold_for_grow_shrink_drag_and_native_resize() {
        let a = LRect { x: -1800.0, y: -1000.0, w: 1600.0, h: 900.0 };
        for (x,y,anchor) in [(-1750.0,-950.0,(0.0,0.0)),(-700.0,-950.0,(1.0,0.0)),
            (-1750.0,-420.0,(0.0,1.0)),(-700.0,-420.0,(1.0,1.0))] {
            let r=LRect{x,y,w:480.0,h:270.0};
            assert_eq!(quadrant_anchor(r,a),anchor);
            let fixed=(r.x+r.w*anchor.0,r.y+r.h*anchor.1);
            for sized in [zoom(r,1.2,a,anchor),zoom(r,0.8,a,anchor),zoom(r,100.0,a,anchor),
                native_resize(r,600.0,350.0,16.0/9.0,a),with_aspect(r,9.0/16.0,a),
                resize(r,(30.0,20.0),(1,1),a)] {
                assert!((sized.x+sized.w*anchor.0-fixed.0).abs()<1e-9);
                assert!((sized.y+sized.h*anchor.1-fixed.1).abs()<1e-9);
                assert!(sized.x>=a.x && sized.y>=a.y && sized.x+sized.w<=a.x+a.w && sized.y+sized.h<=a.y+a.h);
            }
        }
    }
    #[test]
    fn portrait_gestures_do_not_jump_to_a_landscape_minimum_width() {
        let a=LRect{x:0.0,y:0.0,w:1600.0,h:900.0};
        let r=LRect{x:1300.0,y:24.0,w:225.0,h:400.0};
        let anchor=quadrant_anchor(r,a);
        let grown=zoom(r,1.01,a,anchor);
        assert!((grown.w-227.25).abs()<1e-9);
        assert!(zoom(grown,0.99,a,anchor).w<grown.w);
    }
    #[test]
    fn only_valid_intrinsic_dimensions_replace_the_ratio() {
        assert_eq!(stream_aspect(1080.0,1920.0),Some(9.0/16.0));
        for (w,h) in [(0.0,0.0),(1.0,0.0),(-1.0,1.0),(f64::NAN,1.0),(1.0,f64::INFINITY)] {
            assert_eq!(stream_aspect(w,h),None);
        }
    }
    #[test]
    fn gestures_preserve_anchor_and_aspect_without_corner_snap() {
        let r=LRect{x:-1000.0,y:300.0,w:480.0,h:270.0};
        let z=zoom(r,1.01,area(),(0.5,0.5));
        assert!((z.x+z.w/2.0-(r.x+r.w/2.0)).abs()<0.001);
        assert!((z.w/z.h-16.0/9.0).abs()<0.001);
        assert_eq!(zoom(r,1.0,area(),(0.5,0.5)),r);
        assert_eq!(zoom(r,f64::NAN,area(),(0.5,0.5)),r);
        let resized=resize(r,(20.0,10.0),(1,1),area());
        assert_eq!((resized.x+resized.w,resized.y),(r.x+r.w,r.y));
    }
    #[test]
    fn every_size_fits_negative_origin_and_small_work_areas() {
        for a in [area(),LRect{x:100.0,y:-600.0,w:180.0,h:240.0}] {
            for size in [40.0,480.0,5000.0] {
                let r=fit(LRect{x:-9000.0,y:9999.0,w:size,h:size*2.0},a,16.0);
                assert!(r.x>=a.x && r.y>=a.y && r.x+r.w<=a.x+a.w+0.001 && r.y+r.h<=a.y+a.h+0.001);
            }
        }
    }
}
