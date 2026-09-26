#!/usr/bin/env python3
"""Render the Windows installer's wordmark bitmaps (assets/installer).

Inno 6 draws a TBitmapImage from a BMP without scaling it well, so the
installer picks the one rendered for the nearest DPI. Newsreader Italic 500
at display size, ink on paper, like the sidebar wordmark.
Needs Pillow: python3 scripts/installer-art.py
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
FONT = ROOT/'assets/fonts/Newsreader-Italic.ttf'
OUT = ROOT/'assets/installer'
INK, PAPER = (0x14, 0x14, 0x14), (0xff, 0xff, 0xff)
SIZE = 40  # px at 100%

def render(scale):
    font = ImageFont.truetype(str(FONT), round(SIZE*scale/100))
    font.set_variation_by_axes([500, 72])
    left, top, right, bottom = font.getbbox('nus')
    pad = round(2*scale/100)
    image = Image.new('RGB', (right-left+2*pad, bottom-top+2*pad), PAPER)
    ImageDraw.Draw(image).text((pad-left, pad-top), 'nus', font=font, fill=INK)
    image.save(OUT/f'wordmark-{scale}.bmp')

if __name__ == '__main__':
    OUT.mkdir(exist_ok=True)
    for scale in (100, 125, 150, 175, 200):
        render(scale)
