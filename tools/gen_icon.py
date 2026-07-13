"""Regenerate the derived icon assets from the master ``assets/icon.png``.

``icon.png`` is the hand-designed logo and the single source of truth; this
script only re-exports it to the platform icon containers ``icon.ico`` (Windows)
and ``icon.icns`` (macOS) so all three stay in sync. It never draws the mark
itself.

Usage (needs Pillow):
    python3 tools/gen_icon.py
"""

import os

from PIL import Image

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO_ROOT, "assets")

src = Image.open(os.path.join(OUT, "icon.png")).convert("RGBA")

src.save(
    os.path.join(OUT, "icon.ico"),
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
src.save(os.path.join(OUT, "icon.icns"))
print(f"derived icons regenerated in {OUT}")
