#!/usr/bin/env python3
"""Renders the pictures of Oculibre's Windows installer (#17) from the logos:
    python3 scripts/installer-images.py
- desktop/app/src-tauri/windows/installer-header.bmp  150x57, top right of the installer's pages
- desktop/app/src-tauri/windows/installer-sidebar.bmp 164x314, left of its welcome and finish pages
24-bit BMP on white (what NSIS shows). Needs rsvg-convert (librsvg2-bin) and Pillow.
"""
import pathlib
import subprocess
import tempfile

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "desktop/app/src-tauri/windows"


def render(svg, height):
    with tempfile.TemporaryDirectory() as tmp:
        png = pathlib.Path(tmp) / "logo.png"
        subprocess.run(["rsvg-convert", "-h", str(height), str(ROOT / svg), "-o", str(png)], check=True)
        image = Image.open(png).convert("RGBA")
    return image.crop(image.getchannel("A").getbbox())


def picture(size, logo, height, top):
    canvas = Image.new("RGB", size, "white")
    scale = height / logo.height
    logo = logo.resize((round(logo.width * scale), height), Image.LANCZOS)
    x = (size[0] - logo.width) // 2
    canvas.paste(logo, (x, top), logo)
    return canvas


titled = render("oculibre_logo+titre.svg", 600)
picture((150, 57), titled, 45, 6).save(OUT / "installer-header.bmp")
picture((164, 314), titled, 92, 40).save(OUT / "installer-sidebar.bmp")
print("installer-header.bmp, installer-sidebar.bmp")
