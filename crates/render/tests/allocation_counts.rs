#[path = "../../../test-support/count_alloc.rs"]
mod count_alloc;
use nus_render::{text::bundled, FontSystem, Style};

#[test]
fn warmed_text_lookups_do_not_allocate() {
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
    let style = Style {
        font,
        px: 14.0,
        color: [1.0; 4],
        tracking: 0.0,
    };
    let text = "cargo test · café";
    let expected = fonts.shape(font, 14.0, text);
    let width = fonts.measure_as_is(style, text);
    let (_, allocations) = count_alloc::count(|| {
        for _ in 0..1000 {
            let glyphs = fonts.shape(font, 14.0, std::hint::black_box(text));
            assert!(std::sync::Arc::ptr_eq(&expected, &glyphs));
            assert_eq!(fonts.measure_as_is(style, text), width);
        }
    });
    assert_eq!(allocations.calls, 0, "{allocations:?}");
}

#[test]
fn borrowed_keys_preserve_font_size_tracking_and_text_identity() {
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
    let other = fonts.load_bytes(bundled::PLEX_MONO_BOLD, 0).unwrap();
    let a = fonts.shape(font, 14.0, "café");
    for (face, size, text) in [
        (font, 15.0, "café"),
        (other, 14.0, "café"),
        (font, 14.0, "cafe"),
    ] {
        assert!(!std::sync::Arc::ptr_eq(&a, &fonts.shape(face, size, text)));
    }
    let style = Style {
        font,
        px: 14.0,
        color: [1.0; 4],
        tracking: 0.0,
    };
    let base = fonts.measure_as_is(style, "café");
    let spaced = fonts.measure_as_is(
        Style {
            tracking: 2.0,
            ..style
        },
        "café",
    );
    assert!((spaced - base - a.len() as f32 * 2.0).abs() < 0.001);
    assert!(std::sync::Arc::ptr_eq(&a, &fonts.shape(font, 14.0, "café")));
    fonts.reclaim_caches();
    assert!(!std::sync::Arc::ptr_eq(
        &a,
        &fonts.shape(font, 14.0, "café")
    ));
}

#[test]
fn label_allocation_profile() {
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
    let style = Style {
        font,
        px: 14.0,
        tracking: 1.0,
        color: [1.0; 4],
    };
    let label = "NEW TAB · CTRL+SHIFT+D";
    let expected = fonts.measure(style, label);
    let (_, hot) = count_alloc::count(|| {
        for _ in 0..1000 {
            assert_eq!(fonts.measure(style, std::hint::black_box(label)), expected);
        }
    });
    // Pre-size caches, then add new width keys with an already-shaped string.
    for i in 0..1024 {
        fonts.measure_as_is(
            Style {
                tracking: -(i as f32),
                ..style
            },
            label,
        );
    }
    let (_, cold) = count_alloc::count(|| {
        for i in 1024..2024 {
            std::hint::black_box(fonts.measure_as_is(
                Style {
                    tracking: -(i as f32),
                    ..style
                },
                label,
            ));
        }
    });
    assert_eq!(hot.calls, 0);
    assert!(cold.calls < 1100, "cold keys unexpectedly copied: {cold:?}");
    eprintln!("TEXT_ALLOC tracked_1000_calls={} tracked_bytes={} cold_width_1000_calls={} cold_width_bytes={}", hot.calls, hot.bytes, cold.calls, cold.bytes);
}

// Previous UI algorithm, retained here as a behavioral oracle and allocation baseline.
fn legacy_fit(measure: impl Fn(&str) -> f32, text: &str, max_w: f32) -> String {
    if measure(text) <= max_w + 0.01 {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect();
        if measure(&format!("{s}…")) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let s: String = chars[..lo].iter().collect();
    format!("{s}…")
}

#[test]
fn fitting_preserves_unicode_case_and_exact_fit_behavior() {
    use nus_render::text::fit_text;
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::NEWSREADER, 0).unwrap();
    let texts = [
        "",
        "Settings",
        "ffi AVATAR",
        "ÉCOLE İSTANBUL",
        "漢字とかな",
        "cafe\u{301} · naïve",
        "🙂👩‍💻 abc",
        "CTRL+SHIFT+D",
        "  TWO\tWORDS\n",
    ];
    for tracking in [0.0, 1.0] {
        let style = Style {
            font,
            px: 14.0,
            tracking,
            color: [1.0; 4],
        };
        let measure = |s: &str| fonts.measure(style, s);
        for text in texts {
            for width in [
                -1.0,
                0.0,
                2.0,
                15.0,
                33.0,
                75.0,
                measure(text) - 0.005,
                measure(text),
                f32::INFINITY,
            ] {
                assert_eq!(
                    fit_text(text, width, measure),
                    legacy_fit(measure, text, width),
                    "{text:?} at {width}, tracking {tracking}"
                );
            }
        }
    }
}

#[test]
fn fitted_labels_borrow_and_truncation_reuses_its_candidate() {
    use nus_render::text::fit_text;
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
    let style = Style {
        font,
        px: 14.0,
        tracking: 0.0,
        color: [1.0; 4],
    };
    let measure = |s: &str| fonts.measure(style, s);
    let text = "A long workspace label with multiple parts and Unicode café";
    let width = measure(text);
    let expected = fit_text(text, 120.0, measure);
    let (_, fast) = count_alloc::count(|| {
        for _ in 0..1000 {
            assert!(matches!(
                fit_text(text, width, measure),
                std::borrow::Cow::Borrowed(_)
            ));
        }
    });
    let (_, old) = count_alloc::count(|| {
        for _ in 0..1000 {
            assert_eq!(legacy_fit(measure, text, 120.0), expected);
        }
    });
    let (_, new) = count_alloc::count(|| {
        for _ in 0..1000 {
            assert_eq!(fit_text(text, 120.0, measure), expected);
        }
    });
    let owned = text.to_owned();
    let pointer = owned.as_ptr();
    let (result, transferred) = count_alloc::count(|| fit_text(owned, width, measure));
    assert_eq!(result.as_ptr(), pointer);
    assert_eq!(transferred.calls, 0);
    assert_eq!(fast.calls, 0);
    assert_eq!(new.calls, 2000);
    assert!(new.calls < old.calls);
    eprintln!("FIT_ALLOC iterations=1000 fitting_calls={} legacy_truncated_calls={} legacy_truncated_bytes={} truncated_calls={} truncated_bytes={}",fast.calls,old.calls,old.bytes,new.calls,new.bytes);
}

#[test]
fn warmed_tracked_drawing_does_not_allocate() {
    let mut fonts = FontSystem::new();
    let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
    let style = Style {
        font,
        px: 14.0,
        tracking: 1.0,
        color: [1.0; 4],
    };
    let mut scene = nus_render::Scene::new();
    let width = fonts.draw(&mut scene, style, 0.0, 20.0, "NEW TAB · CTRL+SHIFT+D");
    let (_, count) = count_alloc::count(|| {
        for _ in 0..1000 {
            scene.clear();
            assert_eq!(
                fonts.draw(&mut scene, style, 0.0, 20.0, "NEW TAB · CTRL+SHIFT+D"),
                width
            );
        }
    });
    assert_eq!(count.calls, 0, "{count:?}");
}
