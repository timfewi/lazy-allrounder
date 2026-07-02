"""Regenerate the lazy-allrounder icon assets: a voice-waveform mark
(5 rounded bars) in the app's accent blue on a dark disc with a blue ring.

Outputs ``assets/icon.{png,ico,icns}`` relative to the repo root. The bar
heights MUST stay in sync with ``BAR_HEIGHTS`` in
``crates/gui/src/overlay/badge.rs`` so the badge and the window/tray icon read
as the same logo.

Usage (needs Pillow):
    python3 tools/gen_icon.py
"""

import os

from PIL import Image, ImageDraw

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO_ROOT, "assets")

S = 2048  # supersampled canvas, downscaled at the end
u = S / 1024.0
c = S / 2.0

RING = (0x4A, 0x9E, 0xE0, 255)  # theme::ACCENT
DISC = (24, 26, 32, 255)        # theme::SURFACE
BAR = (0x4A, 0x9E, 0xE0, 255)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

outer_r = 480 * u
disc_r = 416 * u
d.ellipse([c - outer_r, c - outer_r, c + outer_r, c + outer_r], fill=RING)
d.ellipse([c - disc_r, c - disc_r, c + disc_r, c + disc_r], fill=DISC)

# Waveform bars — keep in sync with BAR_HEIGHTS in crates/gui/src/overlay/badge.rs
heights = [0.42, 0.72, 1.0, 0.72, 0.42]
bar_w = 76 * u
gap = 52 * u
h_max = 460 * u
for i, h in enumerate(heights):
    x = c + (i - 2) * (bar_w + gap)
    half = h * h_max / 2.0
    d.rounded_rectangle(
        [x - bar_w / 2.0, c - half, x + bar_w / 2.0, c + half],
        radius=bar_w / 2.0,
        fill=BAR,
    )

png512 = img.resize((512, 512), Image.LANCZOS)
png512.save(os.path.join(OUT, "icon.png"))

img.resize((256, 256), Image.LANCZOS).save(
    os.path.join(OUT, "icon.ico"),
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)

png512.save(os.path.join(OUT, "icon.icns"))
print(f"assets regenerated in {OUT}")
