"""Bakes a pixel font into crates/grimora_render/src/font_data.rs, so the engine needs no font
parser: printable ASCII and Latin-1 as rows of bits, the way font8x8 used to come.

    python -I tools/gen_ui_font.py [--preview out.png]

The font must be drawn on a grid of SIZE pixels per em, or the glyphs come out uneven.
"""

import pathlib
import sys

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
FONT = ROOT / "assets/fonts/DepartureMono-Regular.otf"
OUT = ROOT / "crates/grimora_render/src/font_data.rs"
SIZE = 11
BASIC = range(0x20, 0x7F)
LATIN = range(0xA0, 0x100)


def bitmap(font, code, width, height, baseline):
    """The glyph's rows, with bit 0 as the leftmost column."""
    image = Image.new("1", (width, height), 0)
    draw = ImageDraw.Draw(image)
    draw.fontmode = "1"
    draw.text((0, baseline), chr(code), font=font, fill=1, anchor="ls")
    pixels = image.load()
    return [sum(1 << x for x in range(width) if pixels[x, y]) for y in range(height)]


def main():
    font = ImageFont.truetype(str(FONT), SIZE)
    codes = list(BASIC) + list(LATIN)
    boxes = [font.getbbox(chr(code), anchor="ls") for code in codes]
    baseline = -min(box[1] for box in boxes)
    height = baseline + max(box[3] for box in boxes)
    width = max(box[2] for box in boxes)
    if width > 16:
        sys.exit(f"{width} columns don't fit in a u16")

    rows = {code: bitmap(font, code, width, height, baseline) for code in codes}
    empty = [chr(code) for code in codes if code not in (0x20, 0xA0, 0xAD) and not any(rows[code])]
    if empty:
        print("blank glyphs:", " ".join(empty))

    def table(name, span):
        lines = [f"pub(crate) const {name}: [[u16; ROWS]; {len(span)}] = ["]
        for code in span:
            values = ", ".join(str(row) for row in rows[code])
            lines.append(f"    [{values}],")
        lines.append("];")
        return "\n".join(lines)

    OUT.write_text(
        "//! Baked by tools/gen_ui_font.py from assets/fonts/DepartureMono-Regular.otf. Don't edit.\n\n"
        "/// Rows of each glyph, tallest accent to lowest descender.\n"
        f"pub(crate) const ROWS: usize = {height};\n"
        "/// Columns of the widest glyph; bit 0 of a row is its leftmost.\n"
        f"pub(crate) const COLUMNS: usize = {width};\n\n"
        "#[rustfmt::skip]\n" + table("BASIC", BASIC) + "\n\n#[rustfmt::skip]\n" + table("LATIN", LATIN) + "\n",
        encoding="utf-8",
    )
    print(f"{OUT.name}: {width} x {height}, baseline {baseline}")

    if "--preview" in sys.argv:
        lines = ["ABRIR BAÚ  LANÇA DE GELO  FÔLEGO 84/120", "A lua não se põe aqui. Quem entra na fortaleza",
                 "esquece o caminho de volta. Mochila Grimório Diário", "ÃÕÇÉÊÁÍÓÚÂÔÀ ãõçéêáíóúâôà 0123456789 x3 ·?!"]
        sheet = Image.new("RGB", (560, (height + 2) * len(lines) + 4), (10, 12, 26))
        pen = sheet.load()
        for n, line in enumerate(lines):
            x = 4
            for c in line:
                glyph = rows.get(ord(c), rows[ord("?")])
                lit = 0
                for row in glyph:
                    lit |= row
                if lit == 0:
                    x += 4
                    continue
                left = (lit & -lit).bit_length() - 1
                glyph_width = lit.bit_length() - left
                for y, row in enumerate(glyph):
                    for column in range(glyph_width):
                        if row >> (left + column) & 1:
                            pen[x + column, 2 + n * (height + 2) + y] = (232, 226, 200)
                x += glyph_width + 1
        out = sys.argv[sys.argv.index("--preview") + 1]
        sheet.resize((sheet.width * 3, sheet.height * 3), Image.NEAREST).save(out)


if __name__ == "__main__":
    main()
