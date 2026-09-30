#!/usr/bin/env python3
"""Read-only source contracts and independent scalar tests for the Home veil.

This does NOT compile Rust, validate WGSL or execute a GPU. Those tests are in
Scene/home_contrast and crates/render/tests/reading_overlay.rs, respectively.
"""
from __future__ import annotations
import math
from pathlib import Path
import random
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCENE = (ROOT / 'crates/render/src/scene.rs').read_text()
SHADER = (ROOT / 'crates/render/src/quad.wgsl').read_text()
HOME = (ROOT / 'spikes/composite/src/home.rs').read_text()
PALETTE = (ROOT / 'spikes/composite/src/home_contrast.rs').read_text()
KIND23 = SHADER.split('if in.kind == 23u {', 1)[1].split('if in.kind == 22u {', 1)[0]


def constant(name: str) -> float:
    match = re.search(r'const\s+' + re.escape(name) + r'\s*:\s*f32\s*=\s*([0-9.]+)\s*;', PALETTE)
    if match is None:
        raise AssertionError(f'Missing numeric opacity constant {name}')
    return float(match.group(1))


READING = constant('ART_READING_ALPHA')
FOOTER = constant('ART_FOOTER_ALPHA')


def mask(rect: tuple[float, float, float, float], feather: float,
         point: tuple[float, float]) -> float:
    """Independent scalar signed-box/outside-feather oracle, not shader execution."""
    x, y, w, h = rect
    if not all(map(math.isfinite, (*rect, feather))) or w <= 0 or h <= 0:
        return 0.0
    dx = max(x - point[0], 0.0, point[0] - (x + w))
    dy = max(y - point[1], 0.0, point[1] - (y + h))
    distance = math.hypot(dx, dy)
    if feather <= 0:
        return float(distance == 0)
    t = min(1.0, distance / feather)
    return 1.0 - t * t * (3.0 - 2.0 * t)


def opacity(fields, point, alpha=READING):
    return max((mask(r, f, point) * min(1.0, weight)
                for r, f, weight in fields if math.isfinite(weight) and weight > 0),
               default=0.0) * min(1.0, max(0.0, alpha))


class SourceContracts(unittest.TestCase):
    def test_requested_opacity_tokens(self):
        self.assertEqual(READING, .10)
        self.assertEqual(FOOTER, .03)
        self.assertIn('ART_FOOTER_ALPHA / ART_READING_ALPHA', PALETTE)

    def test_art_constructor_changes_background_only(self):
        constructor = PALETTE.split('pub fn for_art(', 1)[1].split('pub fn caret(', 1)[0]
        self.assertIn('Self::new(backdrop, paper, ink, dim, signal)', constructor)
        self.assertIn('palette.veil[3] = ART_READING_ALPHA;', constructor)
        for token in ('primary', 'secondary', 'accent', 'selection', 'selected'):
            self.assertNotRegex(constructor, r'palette\.' + token + r'\s*=')

    def test_plain_home_keeps_protected_surface_path(self):
        plain = HOME.split('p.reading_backdrop=None;', 1)[1].split('let ink=palette.primary;', 1)[0]
        self.assertIn('Palette::new(crate::art::Backdrop::Theme', plain)
        self.assertIn('scene.reading_fields(', plain)
        self.assertNotIn('for_art(', plain)

    def test_art_path_uses_one_weighted_draw(self):
        art = HOME.split('fn draw_home_art(', 1)[1].split('#[cfg(test)]', 1)[0]
        self.assertIn('Palette::for_art(backdrop', art)
        self.assertEqual(art.count('scene.reading_fields_weighted('), 1)
        self.assertIn('(reading,self.px(56.0),1.0)', art)
        self.assertIn('(footer,self.px(20.0),crate::home_contrast::ART_FOOTER_WEIGHT)', art)
        self.assertNotIn('Some(palette.surface)', art)

    def test_kind23_cannot_substitute_hdr_paper(self):
        self.assertNotIn('globals.padding', KIND23)
        self.assertNotIn('in.controls', KIND23)
        self.assertNotIn('in.phase', KIND23)
        self.assertIn('return vec4(in.color.rgb, in.color.a * coverage);', KIND23)

    def test_shader_weighted_max_union(self):
        self.assertIn('clamp(points[at + 2u].y, 0.0, 1.0)', KIND23)
        self.assertIn('coverage = max(coverage, mask * weight);', KIND23)
        self.assertNotIn('coverage +=', KIND23)
        self.assertIn('min(in.raw2, 2u)', KIND23)

    def test_scene_encoding_and_legacy_wrapper_match(self):
        self.assertIn('(r, feather, 1.0)', SCENE)
        self.assertIn('self.points.push([feather, weight]);', SCENE)
        self.assertIn('instance.kind = 23;', SCENE)
        self.assertIn('instance.color2 = count as u32;', SCENE)
        self.assertIn('weight.min(1.0)', SCENE)
        self.assertIn('!weight.is_finite() || weight <= 0.0', SCENE)
        self.assertNotIn('if let Some(surface) = linear_surface', SCENE)

    def test_global_hdr_conversion_and_glyph_paths_still_exist(self):
        fragment = SHADER.split('fn fs_main(', 1)[1]
        self.assertIn('* globals.padding, color.a)', fragment)
        self.assertIn('if in.kind == 20u', fragment)
        self.assertIn('scene.caret(', HOME)
        self.assertIn('palette.selected', HOME)

    def test_space_visibility_has_its_own_closing_delimiter(self):
        source = (ROOT / 'crates/render/src/space.rs').read_text()
        function = source.split('pub fn visibility(', 1)[1].split('#[cfg(test)]', 1)[0]
        # This isolated function contains no strings/comments with braces. This
        # targeted delimiter check is not a general Rust parser or compilation.
        self.assertEqual(function.count('{'), function.count('}'))
        self.assertTrue(function.rstrip().endswith('}\n}'))

    def test_native_gates_target_real_production_sources(self):
        test = (ROOT / 'crates/render/tests/reading_overlay.rs').read_text()
        self.assertIn('include_str!("../src/quad.wgsl")', test)
        self.assertIn('naga::valid::Validator', test)
        self.assertIn('Rgba8Unorm', test)
        self.assertIn('Rgba16Float', test)
        self.assertIn('bytemuck::cast_slice(scene.instances())', test)
        self.assertIn('slice.get_mapped_range()?', test)
        self.assertIn('#[ignore =', test)


class ScalarOracle(unittest.TestCase):
    fields = [((8., 8., 24., 24.), 4., 1.), ((20., 26., 28., 8.), 4., FOOTER/READING)]

    def test_core_footer_overlap(self):
        self.assertAlmostEqual(opacity(self.fields, (12,12)), .10)
        self.assertAlmostEqual(opacity(self.fields, (44,30)), .03)
        self.assertAlmostEqual(opacity(self.fields, (24,28)), .10)
        self.assertLess(opacity(self.fields, (24,28)), 1-(1-.1)*(1-.03))

    def test_corners_have_full_core_coverage(self):
        r=(10.,20.,100.,60.)
        for point in [(10,20),(110,20),(10,80),(110,80),(60,50)]:
            self.assertEqual(mask(r,56,point),1)

    def test_feather_falls_outside_only(self):
        r=(10.,20.,100.,60.)
        self.assertEqual(mask(r,20,(0,50)),.5)
        self.assertEqual(mask(r,20,(-10,50)),0)
        self.assertEqual(mask(r,20,(-11,50)),0)
        values=[mask(r,20,(10-d,50)) for d in range(21)]
        self.assertTrue(all(a>=b for a,b in zip(values,values[1:])))

    def test_gpu_reference_sample_at_pixel_centers(self):
        self.assertAlmostEqual(opacity(self.fields, (5.5,16.5)), .031640625)
        self.assertEqual(opacity(self.fields, (55.5,55.5)),0)

    def test_hidden_footer_and_zero_weight(self):
        f=[((0,0,100,100),20,1), ((0,0,0,0),20,.3)]
        self.assertEqual(opacity(f,(10,10)),.1)
        self.assertEqual(opacity([((0,0,100,100),20,0)],(10,10)),0)
        for invalid in [math.nan,math.inf,-1]:
            self.assertEqual(opacity([((0,0,100,100),20,invalid)],(10,10)),0)

    def test_zero_and_negative_feather_are_hard_fills(self):
        for feather in [0,-10]:
            self.assertEqual(mask((0,0,10,10),feather,(5,5)),1)
            self.assertEqual(mask((0,0,10,10),feather,(-.001,5)),0)

    def test_equivalent_alpha_not_equivalent_encoded_brightness(self):
        # Alpha is identical. Encoded-sRGB and linear-light blending need not
        # produce identical photographed brightness; no such claim is made.
        alpha=opacity(self.fields,(12,12))
        self.assertEqual(alpha,.1)
        self.assertAlmostEqual(1-alpha,.9)
        linear_encoded=1.055*((1-alpha)**(1/2.4))-.055
        self.assertGreater(linear_encoded,1-alpha)
        self.assertGreater(linear_encoded,.95)

    def test_footer_does_not_become_reading_strength_by_itself(self):
        for x in range(40,48):
            self.assertAlmostEqual(opacity(self.fields,(x,30)),.03)

    def test_randomized_geometry_opacity_bound_and_symmetry(self):
        rng=random.Random(20260930)
        for _ in range(10000):
            x,y=rng.uniform(-2000,2000),rng.uniform(-2000,2000)
            w,h=rng.uniform(1,1000),rng.uniform(1,1000)
            f=rng.uniform(0.1,100)
            r=(x,y,w,h)
            p=(rng.uniform(x-f*2,x+w+f*2), rng.uniform(y-f*2,y+h+f*2))
            fields=[(r,f,1),((x+w/2,y+h/2,w,h/2),f*.5,.3)]
            a=opacity(fields,p)
            self.assertGreaterEqual(a,0)
            self.assertLessEqual(a,READING)
            self.assertAlmostEqual(mask(r,f,p),mask(r,f,(2*x+w-p[0],2*y+h-p[1])),places=9)
            self.assertAlmostEqual(opacity([(r,f,1)],(x+w/2,y+h/2)),READING)


if __name__ == '__main__':
    print('Source contracts + scalar oracle only; NOT native/GPU validation.', flush=True)
    unittest.main(verbosity=2)
