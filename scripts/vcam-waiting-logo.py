#!/usr/bin/env python3
"""Renders the logo for the Windows virtual camera's waiting picture (#17), centred on its shutter.

Writes desktop/vcam-dshow/assets/waiting-logo.yuva: SIZE x SIZE pixels, 4 bytes each (Y, U, V,
alpha), BT.601 limited range like the camera's YUY2. Run again when the logo changes:
    python3 scripts/vcam-waiting-logo.py
Needs rsvg-convert (librsvg2-bin) and Pillow.
"""
import pathlib
import subprocess
import tempfile

from PIL import Image

SIZE = 288  # must match LOGO_SIZE in desktop/vcam-dshow/src/picture.rs
ROOT = pathlib.Path(__file__).resolve().parent.parent

with tempfile.TemporaryDirectory() as tmp:
    png = pathlib.Path(tmp) / "logo.png"
    subprocess.run(
        ["rsvg-convert", "-w", str(SIZE), "-h", str(SIZE), "-a", str(ROOT / "oculibre_logo.svg"), "-o", str(png)],
        check=True,
    )
    image = Image.open(png).convert("RGBA")

# Centre the shutter (the blue ring), not the drawing's box: the orange wing and ring stick out
# up and to the right, so the box's centre is not where the eye sees the logo's middle.
pixels = image.load()
blue = [(x, y) for y in range(image.height) for x in range(image.width)
        if pixels[x, y][3] > 128 and pixels[x, y][2] > pixels[x, y][0]]
cx = (min(x for x, _ in blue) + max(x for x, _ in blue)) / 2
cy = (min(y for _, y in blue) + max(y for _, y in blue)) / 2
canvas = Image.new("RGBA", (SIZE, SIZE))
canvas.paste(image, (round(SIZE / 2 - cx), round(SIZE / 2 - cy)), image)
assert image.getchannel("A").getbbox() and canvas.getchannel("A").getbbox()[2] <= SIZE

out = bytearray()
for r, g, b, a in canvas.get_flattened_data():
    y = 16 + (65.738 * r + 129.057 * g + 25.064 * b) / 256
    u = 128 + (-37.945 * r - 74.494 * g + 112.439 * b) / 256
    v = 128 + (112.439 * r - 94.154 * g - 18.285 * b) / 256
    out += bytes([round(y), round(u), round(v), a])
target = ROOT / "desktop/vcam-dshow/assets/waiting-logo.yuva"
target.write_bytes(out)
print(f"{target.relative_to(ROOT)}: {SIZE}x{SIZE}, {len(out)} bytes")
