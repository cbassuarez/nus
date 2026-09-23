//! The Plot boundary identifies local destinations; it does not imply trust.
use nus_render::{Color, Rect, Scene};

pub fn is_local(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else { return false };
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") { return false; }
    match url.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.');
            host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
        }
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
            || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback() || ip.is_private() || ip.is_link_local()),
        None => false,
    }
}

/// Four solid corner brackets and long, quiet survey dashes. Entirely inside
/// the pane; no hit regions, animation, page tint, or change to browser sizing.
pub fn plot(scene: &mut Scene, rect: Rect, scale: f32, ink: Color, paper: Color, compact: bool) {
    let scale = scale.max(0.5);
    let r = rect.inset(scale);
    if r.w < 3.0 * scale || r.h < 3.0 * scale { return; }
    let stroke = scale.max(1.0);
    let corner = (if compact { 5.0 } else { 12.0 } * scale).min(r.w * 0.25).min(r.h * 0.4);
    let mut mark = |x: f32, y: f32, w: f32, h: f32, alpha: f32| {
        let edge = Rect::new(x.round(), y.round(), w.max(stroke), h.max(stroke));
        // A narrow contrasting keyline keeps marks legible over arbitrary sites.
        scene.rect(edge.inset(-stroke).intersect(&rect), [paper[0],paper[1],paper[2],paper[3]*0.65]);
        scene.rect(edge, [ink[0],ink[1],ink[2],ink[3]*alpha]);
    };
    for x in [r.x, r.right()-corner] {
        mark(x, r.y, corner, stroke, 0.85);
        mark(x, r.bottom()-stroke, corner, stroke, 0.85);
    }
    for y in [r.y, r.bottom()-corner] {
        mark(r.x, y, stroke, corner, 0.85);
        mark(r.right()-stroke, y, stroke, corner, 0.85);
    }
    if compact { return; }
    let pitch = 24.0 * scale;
    let dash = 16.0 * scale;
    let mut x = r.x + corner + 8.0 * scale;
    while x < r.right()-corner-4.0*scale {
        let w = dash.min(r.right()-corner-4.0*scale-x);
        mark(x, r.y, w, stroke, 0.38);
        mark(x, r.bottom()-stroke, w, stroke, 0.38);
        x += pitch;
    }
    let mut y = r.y + corner + 8.0 * scale;
    while y < r.bottom()-corner-4.0*scale {
        let h = dash.min(r.bottom()-corner-4.0*scale-y);
        mark(r.x, y, stroke, h, 0.38);
        mark(r.right()-stroke, y, stroke, h, 0.38);
        y += pitch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_parsed_local_hosts_without_matching_paths_or_usernames() {
        for url in ["http://localhost:3000", "https://LOCALHOST./", "http://app.localhost", "http://dev.local", "http://127.10.0.8", "http://[::1]:8080", "http://[fd12::1]", "http://[fe80::1]", "http://[::ffff:127.0.0.1]", "http://10.2.3.4", "http://172.31.2.3", "http://192.168.1.1"] {
            assert!(is_local(url), "{url}");
        }
        for url in ["https://localhost.example.com", "https://localhost@evil.com", "https://example.com/localhost", "http://172.32.1.1", "http://128.0.0.1", "http://[2606:4700::1111]", "file:///localhost/a", "not a URL"] {
            assert!(!is_local(url), "{url}");
        }
    }
    #[test]
    fn boundary_is_confined_to_small_and_large_panes() {
        for (w,h) in [(2.,2.),(7.,7.),(120.,22.),(1400.,900.)] {
            let mut scene=Scene::default();
            let r=Rect::new(101.,57.,w,h);
            plot(&mut scene,r,2.,[0.,0.,0.,1.],[1.;4],false);
            for i in scene.instances() {
                assert!(i.pos[0]>=r.x && i.pos[1]>=r.y);
                assert!(i.pos[0]+i.size[0]<=r.right()+0.01 && i.pos[1]+i.size[1]<=r.bottom()+0.01);
            }
        }
    }
}
