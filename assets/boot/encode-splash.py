"""Encode the PNG as a 128-colour palette and per-row RGB run lengths."""
from itertools import groupby
from pathlib import Path

from PIL import Image

directory = Path(__file__).resolve().parent
with Image.open(directory / "kitty-welcome.png") as source:
    if source.size != (640, 400):
        raise ValueError("the boot image must be 640 × 400")
    indexed = source.convert("RGB").quantize(colors=128)
    data = indexed.tobytes()
    encoded = bytearray(indexed.getpalette()[:128 * 3])
    for y in range(indexed.height):
        row = data[y * indexed.width : (y + 1) * indexed.width]
        for colour, run in groupby(row):
            count = sum(1 for _ in run)
            while count > 255:
                encoded.extend((255, colour))
                count -= 255
            encoded.extend((count, colour))
(directory / "kitty-welcome.rle").write_bytes(encoded)
