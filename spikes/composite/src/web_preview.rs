//! Site identity replaces moving or unavailable media in browser thumbnails.
//! Icons come from the page through the browser, never a bundled brand library.
use nus_render::Rect;

pub(crate) fn streaming_site(address: &str) -> bool {
    if crate::webkit::streaming_service(address) { return true; }
    let Ok(url) = url::Url::parse(address) else { return false };
    if !matches!(url.scheme(), "http" | "https") { return false; }
    let Some(host) = url.host_str() else { return false };
    // These also stream without DRM; their identity is more useful than a
    // moving thumbnail, including before the player has appeared.
    ["youtube.com", "youtu.be", "twitch.tv", "kick.com", "vimeo.com",
     "crunchyroll.com", "spotify.com", "soundcloud.com", "deezer.com",
     "tidal.com", "music.apple.com"].iter().any(|domain| {
        host == *domain || host.strip_suffix(domain).is_some_and(|prefix| prefix.ends_with('.'))
    })
}

pub(crate) fn use_site_icon(page: &crate::browser::Shared) -> bool {
    page.native.is_some() || page.native_ask.is_some() || page.native_video.is_some()
        || page.protected_video || page.video.is_some() || page.media.iter().any(|m| m.blob)
        || streaming_site(&page.url)
        || (page.loading && streaming_site(&page.requested_url))
}

pub(crate) fn same_site(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) if matches!(a.scheme(), "http" | "https") && matches!(b.scheme(), "http" | "https") => a.origin() == b.origin(),
        _ => false,
    }
}

/// Contain the browser-provided icon, preserving its original proportions.
pub(crate) fn icon_rect(bounds: Rect, width: u32, height: u32) -> Rect {
    if width == 0 || height == 0 { return bounds; }
    let scale = (bounds.w / width as f32).min(bounds.h / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    Rect::new(bounds.x + (bounds.w - w) * 0.5, bounds.y + (bounds.h - h) * 0.5, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::Shared;

    #[test]
    fn service_identity_uses_parsed_host_boundaries_and_video_paths() {
        for url in ["https://www.netflix.com/browse", "https://primevideo.com/", "https://www.amazon.co.uk/gp/video/detail/123",
            "https://m.youtube.com/watch?v=1", "https://twitch.tv/channel", "https://open.spotify.com/album/1"] {
            assert!(streaming_site(url), "{url}");
        }
        for url in ["https://netflix.com.example.org/", "https://notyoutube.com/", "https://youtube.com@docs.example.org/",
            "https://example.org/?next=https://netflix.com", "https://amazon.com/gp/videogames", "https://amazon.com/dp/123",
            "file:///youtube.com", "https://tv.apple.com.example.org/"] {
            assert!(!streaming_site(url), "{url}");
        }
    }

    #[test]
    fn preview_suppression_includes_latched_protection_and_pending_native_page() {
        let mut page = Shared { url: "https://docs.example.org/".into(), ..Default::default() };
        assert!(!use_site_icon(&page));
        // Capture permission does not turn an ordinary meeting/document into a stream.
        page.capture_guard = true;
        assert!(!use_site_icon(&page));
        page.protected_video = true;
        assert!(use_site_icon(&page));
        page.url.push_str("#player-removed");
        assert!(use_site_icon(&page));
        page.protected_video = false;
        page.native_ask = Some((1, page.url.clone(), std::time::Instant::now()));
        assert!(use_site_icon(&page));
        page.native_ask = None;
        page.requested_url = "https://netflix.com/".into();
        page.loading = true;
        assert!(use_site_icon(&page));
        page.loading = false;
        assert!(!use_site_icon(&page));
    }

    #[test]
    fn non_square_icons_are_neither_stretched_nor_cropped() {
        let bounds = Rect::new(10.0, 20.0, 48.0, 48.0);
        assert_eq!(icon_rect(bounds, 96, 32), Rect::new(10.0, 36.0, 48.0, 16.0));
        assert_eq!(icon_rect(bounds, 32, 96), Rect::new(26.0, 20.0, 16.0, 48.0));
        assert_eq!(icon_rect(bounds, 64, 64), bounds);
    }

    #[test]
    fn icon_reuse_requires_the_same_http_origin() {
        assert!(same_site("https://example.org/start", "https://example.org/watch/1"));
        assert!(!same_site("https://example.org", "http://example.org"));
        assert!(!same_site("https://example.org", "https://example.org:8443"));
        assert!(!same_site("https://example.org", "https://example.org.evil.test"));
        assert!(!same_site("file:///page", "file:///page"));
    }
}
