#!/usr/bin/env python3
"""Read-only Space asset/data/interface checks. Not Rust or GPU validation."""
from pathlib import Path
import hashlib
import json
import math
import re
import struct
import unittest

ROOT=Path(__file__).resolve().parents[1]
RENDER=ROOT/'crates/render/src'
ART=ROOT/'spikes/composite/assets/art'
SRC=ROOT/'spikes/composite/src'

def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def jpeg_size(data):
    if not data.startswith(b'\xff\xd8'):raise ValueError('not JPEG')
    at=2
    while at<len(data):
        if data[at]!=255:raise ValueError('bad JPEG marker')
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker in (216,217) or 208<=marker<=215:continue
        length=int.from_bytes(data[at:at+2],'big')
        if length<2 or at+length>len(data):raise ValueError('truncated JPEG')
        if marker in (192,193,194):
            return int.from_bytes(data[at+5:at+7],'big'),int.from_bytes(data[at+3:at+5],'big')
        at+=length
    raise ValueError('no JPEG frame')
def stars():
    return list(struct.iter_unpack('<Hhhh',(RENDER/'space-data/stars.bin').read_bytes()))

class SpaceSource(unittest.TestCase):
    def test_asset_hashes_and_dimensions(self):
        manifest=json.loads((ART/'space/manifest.json').read_text())
        for name,record in manifest['images'].items():
            data=(ART/'space'/name).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(),record['sha256'])
            self.assertEqual(list(jpeg_size(data)),record['dimensions'])
        self.assertEqual(digest(RENDER/'space-data/stars.bin'),manifest['catalogue_sha256'])
    def test_source_is_one_native_space_not_a_chart(self):
        s=(ART/'space.luau').read_text()
        self.assertIn('c:orbital({})',s);self.assertIn('c:backdrop("dark")',s)
        for removed in ('local CAT','local FIG','c:celestial','LST','Illustrated northern'):self.assertNotIn(removed,s)
        registry=(SRC/'art.rs').read_text();self.assertEqual(registry.count('("space", "space",'),1)
        self.assertNotIn('("limb",',registry);self.assertNotIn('("darkroom",',registry)
    def test_catalogue_geometry_and_references(self):
        data=stars();self.assertEqual(len(data),9827)
        for ra,dec,mag,bv in data:
            ra=ra/65535*math.tau;dec=dec/32767*math.pi/2
            v=[math.cos(dec)*math.cos(ra),math.cos(dec)*math.sin(ra),math.sin(dec)]
            self.assertAlmostEqual(sum(x*x for x in v),1.0,places=12)
            self.assertTrue(-10<mag/100<20)
        source=(RENDER/'space_catalog.rs').read_text()
        pairs=re.findall(r'^\s*\[(\d+), (\d+)\],',source,re.M)
        self.assertEqual(len(pairs),674)
        self.assertTrue(all(int(n)<len(data) for pair in pairs for n in pair))
        self.assertEqual(len(re.findall(r'^\s*\("',source,re.M)),88)
    def test_shader_uniform_and_vertex_contract(self):
        shader=(RENDER/'space.wgsl').read_text();host=(RENDER/'space.rs').read_text()
        self.assertIn('resolution: vec4<f32>',shader);self.assertIn('reading: vec4<f32>',shader)
        self.assertEqual(set(map(int,re.findall(r'@binding\((\d+)\)',shader))),set(range(9)))
        for method in ['dust_main','scene_main','star_vs','star_fs','line_vs','line_fs','edge_fs']:
            self.assertIn('fn '+method+'(',shader);self.assertIn('"'+method+'"',host)
        self.assertIn('mipmap_filter: wgpu::MipmapFilterMode::Linear',host)
        self.assertIn('depth_slice: None',host)
    def test_images_are_not_sampled_with_srgb_double_conversion(self):
        s=(RENDER/'space.rs').read_text()
        self.assertIn('format: wgpu::TextureFormat::Rgba8Unorm',s)
        self.assertNotIn('Rgba8UnormSrgb',s)
        self.assertIn('let albedo=pow(srgb,vec3(2.2))',(RENDER/'space.wgsl').read_text())
    def test_idle_reuses_do_not_submit(self):
        s=(RENDER/'space.rs').read_text()
        self.assertLess(s.index('self.previous == Some(p)'),s.index('res.queue.write_buffer'))
        self.assertIn('if dust_dirty',s);self.assertIn('old.seed != p.seed',s)
        self.assertIn('native_space {',(SRC/'home.rs').read_text())
        self.assertIn('!self.motion.reduced() && !native_space',(SRC/'settings.rs').read_text())
    def test_controls_have_real_input_and_accessibility_routes(self):
        self.assertIn('self.space_key(ev)',(SRC/'home.rs').read_text())
        self.assertIn('self.space_action(id,action)',(SRC/'home.rs').read_text())
        access=(SRC/'access.rs').read_text()
        self.assertIn('Target::Space(h.art_id,*action)',access);self.assertIn('Target::SpacePrompt(h.art_id)',access)
        self.assertIn('SpacePrompt(id)',access)
    def test_api_budget_and_legacy_migration(self):
        s=(SRC/'art.rs').read_text()
        self.assertIn('at most two orbital views',s);self.assertIn('self.key=="space" && retired_space(src)',s)
        self.assertIn('Sha256::digest(source.as_bytes())',s)
        self.assertNotIn('remove_file',s[s.index('fn retired_space'):s.index('fn load(&mut self')])
    def test_no_browser_or_html_in_new_artwork(self):
        for path in [SRC/'space.rs',RENDER/'space.rs',RENDER/'space_motion.rs']:
            s=path.read_text()
            for unwanted in ['BrowserTab::','create_browser','include_str!("limb-darkroom','Command::new','WebView::']:
                self.assertNotIn(unwanted,s)
    def test_normalized_camera_formula_matches_reference(self):
        # Equation-level check only: GL bottom-left vs native top-left mapping.
        for w,h in [(1100,570),(540,780),(300,180),(2400,1200)]:
            for x,y in [(0.5,0.5),(w*.4,h*.6),(w-.5,h-.5)]:
                old=[(x-.5*w)/h,((h-y)-.5*h)/h,-2.0]
                new=[(x-.5*w)/h,(.5*h-y)/h,-2.0]
                self.assertTrue(all(abs(a-b)<1e-12 for a,b in zip(old,new)))
    def test_reading_material_stays_outside_native_art(self):
        s=(SRC/'space.rs').read_text()
        self.assertNotIn('reading_fields(',s);self.assertNotIn('reading_field(',s)
        self.assertNotIn('bv-reading-shade',s)
    def test_shader_bounds_and_output_alpha(self):
        s=(RENDER/'space.wgsl').read_text()
        self.assertIn('clamp(dot(n,-rd),0.,1.)',s)
        self.assertIn('if(q.z<=.01)',s)
        self.assertIn('vec4(sky_color(pixel),1.)',s)
    def test_app_and_dll_share_module(self):
        self.assertEqual((SRC/'main.rs').read_text().count('\nmod space;'),1)
        self.assertIn('main.rs',(SRC/'lib.rs').read_text())
    def test_attribution_is_embedded_and_preserved(self):
        note=(ART/'space/NOTICE.md').read_text()
        for name in ('NASA','Stellarium','Hipparcos','CC BY-SA','historical'):
            self.assertIn(name.lower(),note.lower())
        self.assertIn('ATTRIBUTION.into()',(SRC/'space.rs').read_text())
        self.assertEqual(note,(RENDER/'space-data/NOTICE.md').read_text())

if __name__=='__main__':unittest.main(verbosity=2)
