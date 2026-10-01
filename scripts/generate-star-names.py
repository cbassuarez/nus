#!/usr/bin/env python3
"""Name the packed Hipparcos catalogue's stars from the IAU Catalog of Star Names.

Sources (copied under crates/render/src/space-data/names/, see NOTICE.md there):
  IAU-CSN.txt          IAU WGSN star names (name, Bayer, constellation, HIP)
  hip_main_named.tsv   VizieR I/239/hip_main: HIP, RAICRS, DEICRS (J1991.25), Vmag, SpType
  hip2_named.tsv       VizieR I/311/hip2: HIP, Plx, e_Plx (van Leeuwen 2007)

A name is attached to a catalogue index by position (the catalogue is the same
epoch, quantised to about 10 arcsec) and brightness, never by guesswork.
Default is --check (read-only). --write changes only space_names.rs.
"""
from pathlib import Path
import argparse
import json
import math
import re
import struct

ROOT = Path(__file__).resolve().parents[1]
HERE = ROOT / 'crates/render/src'
DATA = HERE / 'space-data/names'

COLS = ['Name/ASCII', 'Name/Diacritics', 'Designation', 'ID', 'ID2', 'Con', '#', 'WDS_J',
        'mag', 'bnd', 'HIP', 'HD', 'RA', 'Dec', 'Date', 'Notes']


def read_iau():
    out = []
    for line in (DATA / 'IAU-CSN.txt').read_text(encoding='utf-8').splitlines():
        if not line.strip() or line[0] in '#$':
            continue
        # The three name cells are fixed-width (names hold spaces); every later
        # cell is a single token, "_" when empty.
        cells = [line[0:17].strip(), line[17:35].strip(), line[35:48].strip()] + line[48:].split()
        if len(cells) < 15:
            continue
        row = dict(zip(COLS, cells))
        if row['HIP'].isdigit():
            out.append(row)
    return out


def read_tsv(name):
    rows, header = {}, None
    for line in (DATA / name).read_text(encoding='utf-8').splitlines():
        if not line.strip() or line[0] == '#':
            continue
        cells = [c.strip() for c in line.split('\t')]
        if header is None:
            header = cells
            continue
        if cells[0].isdigit():
            rows[int(cells[0])] = dict(zip(header, cells))
    return rows


def catalogue():
    raw = (HERE / 'space-data/stars.bin').read_bytes()
    out = []
    for ra, dec, v, bv in struct.iter_unpack('<Hhhh', raw):
        r, d = ra / 65535 * math.tau, dec / 32767 * math.pi / 2
        out.append(((math.cos(d) * math.cos(r), math.cos(d) * math.sin(r), math.sin(d)), v / 100))
    return out


def direction(ra_deg, dec_deg):
    r, d = math.radians(ra_deg), math.radians(dec_deg)
    return (math.cos(d) * math.cos(r), math.cos(d) * math.sin(r), math.sin(d))


# IAU abbreviation -> (name, Latin genitive), the 88 constellations.
CON = {
    'And': ('Andromeda', 'Andromedae'), 'Ant': ('Antlia', 'Antliae'), 'Aps': ('Apus', 'Apodis'),
    'Aqr': ('Aquarius', 'Aquarii'), 'Aql': ('Aquila', 'Aquilae'), 'Ara': ('Ara', 'Arae'),
    'Ari': ('Aries', 'Arietis'), 'Aur': ('Auriga', 'Aurigae'), 'Boo': ('Boötes', 'Boötis'),
    'Cae': ('Caelum', 'Caeli'), 'Cam': ('Camelopardalis', 'Camelopardalis'), 'Cnc': ('Cancer', 'Cancri'),
    'CVn': ('Canes Venatici', 'Canum Venaticorum'), 'CMa': ('Canis Major', 'Canis Majoris'),
    'CMi': ('Canis Minor', 'Canis Minoris'), 'Cap': ('Capricornus', 'Capricorni'),
    'Car': ('Carina', 'Carinae'), 'Cas': ('Cassiopeia', 'Cassiopeiae'), 'Cen': ('Centaurus', 'Centauri'),
    'Cep': ('Cepheus', 'Cephei'), 'Cet': ('Cetus', 'Ceti'), 'Cha': ('Chamaeleon', 'Chamaeleontis'),
    'Cir': ('Circinus', 'Circini'), 'Col': ('Columba', 'Columbae'), 'Com': ('Coma Berenices', 'Comae Berenices'),
    'CrA': ('Corona Australis', 'Coronae Australis'), 'CrB': ('Corona Borealis', 'Coronae Borealis'),
    'Crv': ('Corvus', 'Corvi'), 'Crt': ('Crater', 'Crateris'), 'Cru': ('Crux', 'Crucis'),
    'Cyg': ('Cygnus', 'Cygni'), 'Del': ('Delphinus', 'Delphini'), 'Dor': ('Dorado', 'Doradus'),
    'Dra': ('Draco', 'Draconis'), 'Equ': ('Equuleus', 'Equulei'), 'Eri': ('Eridanus', 'Eridani'),
    'For': ('Fornax', 'Fornacis'), 'Gem': ('Gemini', 'Geminorum'), 'Gru': ('Grus', 'Gruis'),
    'Her': ('Hercules', 'Herculis'), 'Hor': ('Horologium', 'Horologii'), 'Hya': ('Hydra', 'Hydrae'),
    'Hyi': ('Hydrus', 'Hydri'), 'Ind': ('Indus', 'Indi'), 'Lac': ('Lacerta', 'Lacertae'),
    'Leo': ('Leo', 'Leonis'), 'LMi': ('Leo Minor', 'Leonis Minoris'), 'Lep': ('Lepus', 'Leporis'),
    'Lib': ('Libra', 'Librae'), 'Lup': ('Lupus', 'Lupi'), 'Lyn': ('Lynx', 'Lyncis'),
    'Lyr': ('Lyra', 'Lyrae'), 'Men': ('Mensa', 'Mensae'), 'Mic': ('Microscopium', 'Microscopii'),
    'Mon': ('Monoceros', 'Monocerotis'), 'Mus': ('Musca', 'Muscae'), 'Nor': ('Norma', 'Normae'),
    'Oct': ('Octans', 'Octantis'), 'Oph': ('Ophiuchus', 'Ophiuchi'), 'Ori': ('Orion', 'Orionis'),
    'Pav': ('Pavo', 'Pavonis'), 'Peg': ('Pegasus', 'Pegasi'), 'Per': ('Perseus', 'Persei'),
    'Phe': ('Phoenix', 'Phoenicis'), 'Pic': ('Pictor', 'Pictoris'), 'Psc': ('Pisces', 'Piscium'),
    'PsA': ('Piscis Austrinus', 'Piscis Austrini'), 'Pup': ('Puppis', 'Puppis'), 'Pyx': ('Pyxis', 'Pyxidis'),
    'Ret': ('Reticulum', 'Reticuli'), 'Sge': ('Sagitta', 'Sagittae'), 'Sgr': ('Sagittarius', 'Sagittarii'),
    'Sco': ('Scorpius', 'Scorpii'), 'Scl': ('Sculptor', 'Sculptoris'), 'Sct': ('Scutum', 'Scuti'),
    'Ser': ('Serpens', 'Serpentis'), 'Sex': ('Sextans', 'Sextantis'), 'Tau': ('Taurus', 'Tauri'),
    'Tel': ('Telescopium', 'Telescopii'), 'Tri': ('Triangulum', 'Trianguli'),
    'TrA': ('Triangulum Australe', 'Trianguli Australis'), 'Tuc': ('Tucana', 'Tucanae'),
    'UMa': ('Ursa Major', 'Ursae Majoris'), 'UMi': ('Ursa Minor', 'Ursae Minoris'),
    'Vel': ('Vela', 'Velorum'), 'Vir': ('Virgo', 'Virginis'), 'Vol': ('Volans', 'Volantis'),
    'Vul': ('Vulpecula', 'Vulpeculae'),
}


def rust_str(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


def build():
    iau, main, hip2 = read_iau(), read_tsv('hip_main_named.tsv'), read_tsv('hip2_named.tsv')
    stars = catalogue()
    named, seen = [], set()
    for row in iau:
        hip = int(row['HIP'])
        m = main.get(hip)
        if m is None or hip in seen:
            continue
        try:
            ra, dec, vmag = float(m['RAICRS']), float(m['DEICRS']), float(m['Vmag'])
        except ValueError:
            continue
        want = direction(ra, dec)
        best, best_sep = None, 1e9
        for i, (d, v) in enumerate(stars):
            if abs(v - vmag) > 0.16:
                continue
            sep = math.acos(max(-1.0, min(1.0, sum(a * b for a, b in zip(d, want)))))
            if sep < best_sep:
                best, best_sep = i, sep
        # 0.02 degrees is twice the catalogue's own position quantisation.
        if best is None or best_sep > math.radians(0.02):
            continue
        seen.add(hip)
        p = hip2.get(hip, {})
        try:
            plx, err = float(p.get('Plx', '')), float(p.get('e_Plx', ''))
        except ValueError:
            plx = err = 0.0
        con = CON.get(row['Con'])
        designation = constellation = ''
        if con:
            constellation = con[0]
            # The Greek letter if the IAU list has one, else its Flamsteed number.
            letter = row['ID2'] if row['ID2'] not in ('', '_') else row['ID']
            if letter not in ('', '_'):
                designation = f'{letter} {con[1]}'
        named.append((best, row['Name/Diacritics'] or row['Name/ASCII'], designation, constellation,
                      hip, plx, err, m.get('SpType', '').strip()))
    named.sort(key=lambda n: n[0])
    figures = json.loads((HERE / 'space-data/constellation-figures.json').read_text(encoding='utf-8'))
    lines = ['// Generated by scripts/generate-star-names.py. Do not hand-edit.',
             '// IAU Catalog of Star Names (WGSN); Hipparcos I/239 and I/311 via CDS VizieR.',
             '// Each entry points at the packed catalogue by index; see space-data/names/.',
             '/// star index, IAU name, designation ("α Canis Majoris"), constellation, HIP number,',
             '/// parallax and its error in mas (0 when unknown), spectral type as catalogued.',
             'pub(super) const NAMED: &[(u16, &str, &str, &str, u32, f32, f32, &str)] = &[']
    for idx, name, bayer, con, hip, plx, err, sp in named:
        lines.append(f'    ({idx}, {rust_str(name)}, {rust_str(bayer)}, {rust_str(con)}, {hip}, {plx:.2f}, {err:.2f}, {rust_str(sp)}),')
    lines.append('];')
    # The figures in catalogue order: IAU abbreviation, name, the segments they own
    # in space_catalog.rs's LINES (first, one past last), and a point to hang a name on.
    lines += ['',
              '/// abbreviation, name, first and one-past-last segment of LINES, label anchor (J2000 unit vector).',
              'pub(super) const FIGURES: &[(&str, &str, u16, u16, [f32; 3])] = &[']
    start = 0
    for figure in figures:
        n = len(figure['lines'])
        if figure['anchor'] is None:
            ids = list(dict.fromkeys(i for pair in figure['lines'] for i in pair))
            v = [sum(stars[i][0][axis] for i in ids) for axis in range(3)]
            length = math.sqrt(sum(x * x for x in v))
            v = [x / length for x in v]
        else:
            v = list(stars[figure['anchor']][0])
        lines.append(f"    ({rust_str(figure['id'])}, {rust_str(figure['name'])}, {start}, {start + n}, [{v[0]:.8f}, {v[1]:.8f}, {v[2]:.8f}]),")
        start += n
    lines.append('];')
    return '\n'.join(lines) + '\n', len(named), len(iau)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--write', action='store_true')
    args = p.parse_args()
    target = HERE / 'space_names.rs'
    rendered, matched, total = build()
    if args.write:
        target.write_text(rendered, encoding='utf-8', newline='\n')
        print(f'Wrote {target}: {matched} of {total} IAU names with Hipparcos numbers matched')
    elif not target.exists() or target.read_text(encoding='utf-8') != rendered:
        raise SystemExit('Generated names differ; review before running --write')
    else:
        print(f'PASS: star names regenerated byte-for-byte ({matched} stars)')


if __name__ == '__main__':
    main()
