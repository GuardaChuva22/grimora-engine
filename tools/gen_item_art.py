"""Draws the items that have no model to photograph: their icon, 48 pixels, and their piece for
the inventory's grid, 24 texels per cell. Run with `python -I tools/gen_item_art.py`.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "assets" / "items" / "icons"
PIECES = ROOT / "assets" / "items" / "pieces"

GOLD, GOLD_LIT, GOLD_DARK = (196, 150, 66, 255), (240, 210, 130, 255), (104, 72, 34, 255)
IRON, IRON_LIT, IRON_DARK = (112, 116, 128, 255), (172, 176, 186, 255), (52, 54, 66, 255)
LEAF, LEAF_LIT, LEAF_DARK = (88, 128, 70, 255), (146, 180, 96, 255), (40, 66, 44, 255)
GLASS, GLASS_LIT, WINE = (96, 112, 132, 255), (190, 206, 214, 255), (140, 44, 62, 255)
CLOTH, CLOTH_LIT, CLOTH_DARK = (84, 66, 112, 255), (128, 104, 150, 255), (44, 34, 66, 255)
HIDE, HIDE_LIT, HIDE_DARK = (120, 84, 56, 255), (164, 122, 82, 255), (66, 44, 34, 255)


def moeda(d, w, h):
    d.ellipse((w * 0.14, h * 0.14, w * 0.86, h * 0.86), fill=GOLD_DARK)
    d.ellipse((w * 0.14, h * 0.1, w * 0.84, h * 0.8), fill=GOLD)
    d.ellipse((w * 0.26, h * 0.2, w * 0.72, h * 0.68), outline=GOLD_DARK)
    d.arc((w * 0.2, h * 0.14, w * 0.78, h * 0.74), 170, 280, fill=GOLD_LIT)


def sucata(d, w, h):
    d.polygon([(w * .1, h * .7), (w * .36, h * .26), (w * .62, h * .4), (w * .46, h * .84)], fill=IRON_DARK)
    d.polygon([(w * .14, h * .64), (w * .38, h * .22), (w * .58, h * .36), (w * .4, h * .74)], fill=IRON)
    d.line([(w * .2, h * .58), (w * .38, h * .28)], fill=IRON_LIT)
    d.polygon([(w * .52, h * .78), (w * .68, h * .3), (w * .9, h * .42), (w * .78, h * .86)], fill=IRON_DARK)
    d.polygon([(w * .56, h * .7), (w * .7, h * .28), (w * .86, h * .38), (w * .74, h * .76)], fill=IRON)


def erva(d, w, h):
    d.line([(w * .5, h * .92), (w * .5, h * .2)], fill=LEAF_DARK, width=max(1, w // 24))
    for y, side in ((.68, -1), (.56, 1), (.42, -1), (.3, 1)):
        tip = (w * (.5 + side * .36), h * (y - .14))
        d.polygon([(w * .5, h * y), (w * (.5 + side * .18), h * (y - .14)), tip, (w * (.5 + side * .2), h * y)], fill=LEAF)
        d.line([(w * .5, h * y), tip], fill=LEAF_LIT)
    d.ellipse((w * .44, h * .08, w * .56, h * .22), fill=LEAF_LIT)


def tonico(d, w, h):
    d.rectangle((w * .4, h * .06, w * .6, h * .16), fill=HIDE)
    d.rectangle((w * .42, h * .16, w * .58, h * .3), fill=GLASS)
    d.rounded_rectangle((w * .2, h * .3, w * .8, h * .94), radius=w * .14, fill=GLASS)
    d.rounded_rectangle((w * .25, h * .48, w * .75, h * .9), radius=w * .1, fill=WINE)
    d.line([(w * .3, h * .36), (w * .3, h * .82)], fill=GLASS_LIT, width=max(1, w // 16))


def capuz(d, w, h):
    d.polygon([(w * .5, h * .06), (w * .84, h * .42), (w * .9, h * .92), (w * .1, h * .92), (w * .16, h * .42)], fill=CLOTH)
    d.polygon([(w * .5, h * .06), (w * .84, h * .42), (w * .9, h * .92), (w * .66, h * .92), (w * .6, h * .4)], fill=CLOTH_DARK)
    d.ellipse((w * .3, h * .36, w * .7, h * .84), fill=(16, 14, 28, 255))
    d.arc((w * .28, h * .32, w * .72, h * .86), 180, 320, fill=CLOTH_LIT)


def gibao(d, w, h):
    d.polygon([(w * .24, h * .06), (w * .76, h * .06), (w * .96, h * .24), (w * .84, h * .42), (w * .76, h * .36),
               (w * .76, h * .94), (w * .24, h * .94), (w * .24, h * .36), (w * .16, h * .42), (w * .04, h * .24)], fill=HIDE)
    d.polygon([(w * .4, h * .06), (w * .5, h * .18), (w * .6, h * .06)], fill=(16, 14, 28, 255))
    d.line([(w * .5, h * .18), (w * .5, h * .94)], fill=HIDE_DARK, width=max(1, w // 24))
    for y in (.32, .46, .6, .74):
        d.line([(w * .44, h * y), (w * .56, h * y)], fill=IRON_LIT)
    d.line([(w * .28, h * .1), (w * .28, h * .9)], fill=HIDE_LIT)


def botas(d, w, h):
    def boot(x, top, bottom):
        d.polygon([(w * x, h * top), (w * (x + .24), h * top), (w * (x + .24), h * (bottom - .2)),
                   (w * (x + .4), h * (bottom - .12)), (w * (x + .4), h * bottom), (w * x, h * bottom)], fill=HIDE_DARK)
        d.polygon([(w * x, h * top), (w * (x + .2), h * top), (w * (x + .2), h * (bottom - .2)),
                   (w * (x + .36), h * (bottom - .1)), (w * (x + .36), h * (bottom - .06)), (w * x, h * (bottom - .06))], fill=HIDE)
        d.line([(w * x, h * (top + .08)), (w * (x + .22), h * (top + .08))], fill=HIDE_LIT)

    boot(.08, .1, .84)
    boot(.5, .16, .92)


ITEMS = {
    "moeda": (moeda, (1, 1)),
    "sucata_de_ferro": (sucata, (1, 1)),
    "erva_amarga": (erva, (1, 1)),
    "tonico": (tonico, (1, 2)),
    "capuz": (capuz, (2, 2)),
    "gibao": (gibao, (2, 3)),
    "botas": (botas, (2, 2)),
}


def picture(draw, width, height):
    image = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    draw(ImageDraw.Draw(image), width, height)
    return image


for name, (draw, (columns, rows)) in ITEMS.items():
    piece = picture(draw, columns * 24, rows * 24)
    piece.save(PIECES / f"{name}.png")
    # The icon keeps the piece's shape inside its square.
    icon = Image.new("RGBA", (48, 48), (0, 0, 0, 0))
    fit = 48 // max(columns, rows)
    drawn = picture(draw, columns * fit, rows * fit)
    icon.alpha_composite(drawn, ((48 - drawn.width) // 2, (48 - drawn.height) // 2))
    icon.save(ICONS / f"{name}.png")
    print(name)
