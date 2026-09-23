//! Pure desktop geometry. Pointer gestures resize around a fixed anchor;
//! clamping never relocates a window to a different corner.
use super::LRect;

pub fn stream_aspect(width: f64, height: f64) -> Option<f64> {
    let ratio = width / height;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0
        && ratio.is_finite() && ratio > 0.0).then_some(ratio)
}

/// Keep the chosen width and center when metadata changes the stream shape.
pub fn with_aspect(r: LRect, aspect: f64, area: LRect) -> LRect {
    let h = r.w / aspect;
    fit(LRect { y: r.y + (r.h - h) / 2.0, h, ..r }, area, 16.0)
}

/// Native resize proposals may change either dimension. Honor the dimension
/// that moved most, then restore the stream ratio before the next gesture.
pub fn native_resize(previous: LRect, w: f64, h: f64, aspect: f64, area: LRect) -> LRect {
    let w = if (w - previous.w).abs() >= (h - previous.h).abs() * aspect { w } else { h * aspect };
    fit(LRect { w, h: w / aspect, ..previous }, area, 16.0)
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
    let max_w = (area.w - 32.0).min((area.h - 32.0) * aspect).max(1.0);
    let min_w = 240.0_f64.min(max_w);
    let w = (r.w * factor).clamp(min_w, max_w);
    let h = w / aspect;
    fit(LRect { x: r.x + (r.w - w) * anchor.0, y: r.y + (r.h - h) * anchor.1, w, h }, area, 16.0)
}

/// Opposite corner remains fixed during an edge/corner drag.
pub fn resize(r: LRect, delta: (f64, f64), edge: (i8, i8), area: LRect) -> LRect {
    let dx = delta.0 * edge.0 as f64;
    let dy = delta.1 * edge.1 as f64 * r.w / r.h.max(1.0);
    let change = if edge.0 == 0 {dy} else if edge.1 == 0 || dx.abs() >= dy.abs() {dx} else {dy};
    let anchor = (if edge.0 < 0 {1.0} else if edge.0 > 0 {0.0} else {0.5}, if edge.1 < 0 {1.0} else if edge.1 > 0 {0.0} else {0.5});
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
        assert_eq!((resized.x,resized.y),(r.x,r.y));
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
