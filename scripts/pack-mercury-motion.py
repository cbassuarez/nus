#!/usr/bin/env python3
"""Pack deterministic desktop animation frames as losslessly compressed PNGs.

Generate inputs with composite's `mercury` example first. This changes PNG
compression only; all RGBA samples are preserved. Requires Pillow.
"""
from pathlib import Path
import io
import struct
import sys
from PIL import Image

root = Path(sys.argv[1])
output = Path(sys.argv[2])
intro = sorted((root / 'intro').glob('*.png'))
idle = []  # The Dock settles; only the claim material keeps flowing.
assert len(intro) == 84 and len(idle) == 0
blob = bytearray(b'NUSM\x01\x00\x00\x00' + struct.pack('<II', len(intro), len(idle)))
for path in intro + idle:
    source = Image.open(path).convert('RGBA')
    assert source.size == (256, 256)
    png = io.BytesIO()
    source.save(png, format='PNG', optimize=True)
    data = png.getvalue()
    assert Image.open(io.BytesIO(data)).tobytes() == source.tobytes()
    blob += struct.pack('<I', len(data)) + data
output.write_bytes(blob)
print(f'{len(intro)} intro + {len(idle)} liquid frames: {len(blob):,} bytes')
