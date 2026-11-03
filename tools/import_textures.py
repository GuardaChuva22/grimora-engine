"""Brings pixel art into assets/textures/, ready for scenes and the engine.

    python -I tools/import_textures.py "C:/caminho/da/pasta"

Every PNG in the folder and in moons/ is copied as RGBA under an ASCII lowercase
name (`água.png` becomes `agua.png`), with moons/ kept as a subfolder. Other
subfolders are left alone. Transparent pixels get the color of the nearest drawn
one, still transparent, so that filtering and mipmaps show no dark fringe. Run it
again whenever the folder has something new; existing files are replaced.

Needs Pillow.
"""

import sys
import unicodedata
from collections import deque
from pathlib import Path

from PIL import Image

OUT = Path(__file__).resolve().parent.parent / "assets" / "textures"
SUBFOLDERS = ["moons"]


def ascii_name(name):
    plain = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()
    return plain.lower().replace(" ", "_")


def bleed(image):
    """Gives transparent pixels the color of the nearest drawn one, alpha still 0."""
    w, h = image.size
    pixels = image.load()
    queue = deque((x, y) for y in range(h) for x in range(w) if pixels[x, y][3] > 0)
    seen = set(queue)
    while queue:
        x, y = queue.popleft()
        r, g, b, _ = pixels[x, y]
        for nx, ny in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]:
            if 0 <= nx < w and 0 <= ny < h and (nx, ny) not in seen:
                seen.add((nx, ny))
                pixels[nx, ny] = (r, g, b, 0)
                queue.append((nx, ny))


def import_folder(source, target):
    target.mkdir(parents=True, exist_ok=True)
    for path in sorted(source.glob("*.png")):
        image = Image.open(path).convert("RGBA")
        transparent = image.getchannel("A").getextrema()[0] == 0
        if transparent:
            bleed(image)
        image.save(target / ascii_name(path.name))
        print(f"{ascii_name(path.name)}: {image.width}x{image.height}{' (sprite)' if transparent else ''}")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    source = Path(sys.argv[1])
    import_folder(source, OUT)
    for folder in SUBFOLDERS:
        import_folder(source / folder, OUT / folder)


if __name__ == "__main__":
    main()
