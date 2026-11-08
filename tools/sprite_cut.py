"""Cut an imagegen sprite sheet (objects on flat magenta) into crisp pixel-art sprites.

Steps: key out the magenta (and its anti-aliased fringe) -> split objects at empty columns (or rows) -> crop each to its
bounding box -> scale to a real pixel height with a box filter -> quantize to a small palette -> hard alpha -> 1 px dark
outline. Writes <out>/<name>_<i>.png and a 6x contact sheet <out>/<name>_preview.png to look at before packing.

    python tools/sprite_cut.py <sheet.png> <name> --height 22 [--out assets/atlas/src] [--colors 16] [--axis x]
"""
import argparse
import os
import sys

from PIL import Image

OUTLINE = (24, 20, 32, 255)


def is_key(r, g, b):
    # Pure magenta and its fringe: red and blue high, green well below both.
    return r > 150 and b > 150 and g < min(r, b) - 70


def key_out(im):
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_key(r, g, b):
                px[x, y] = (0, 0, 0, 0)
            elif r > 120 and b > 120 and g < min(r, b) - 30:
                # Despill: a pinkish edge pixel keeps its shape but loses the magenta cast.
                m = max(g, min(r, b) - 40)
                px[x, y] = (min(r, m + 40), g, min(b, m + 40), a)
    return im


def spans(im, axis, min_gap):
    """Runs of non-empty columns (axis x) or rows (axis y), split where >= min_gap empty lines in a row."""
    w, h = im.size
    alpha = im.getchannel("A").load()
    n = w if axis == "x" else h
    filled = []
    for i in range(n):
        if axis == "x":
            filled.append(any(alpha[i, y] > 0 for y in range(0, h, 2)))
        else:
            filled.append(any(alpha[x, i] > 0 for x in range(0, w, 2)))
    out, start, gap = [], None, 0
    for i, f in enumerate(filled + [False] * (min_gap + 1)):
        if f:
            if start is None:
                start = i
            gap = 0
        elif start is not None:
            gap += 1
            if gap >= min_gap:
                out.append((start, i - gap + 1))
                start, gap = None, 0
    return out


def grid_sample(piece, block):
    """The source is already pixel art drawn on a ~block-px grid: take each block's centre pixel (no blur)."""
    w, h = piece.size
    out = Image.new("RGBA", (max(1, w // block), max(1, h // block)))
    src, dst = piece.load(), out.load()
    for y in range(out.size[1]):
        for x in range(out.size[0]):
            dst[x, y] = src[min(w - 1, x * block + block // 2), min(h - 1, y * block + block // 2)]
    return out


def pixelate(piece, height, colors, scale=None, block=0):
    bbox = piece.getbbox()
    piece = piece.crop(bbox)
    w, h = piece.size
    if block:
        small = grid_sample(piece, block)
    else:
        scale = scale if scale else height / h
        small = piece.resize((max(1, round(w * scale)), max(1, round(h * scale))), Image.BOX)
    # Magenta-tinted leftovers (thin lines the key couldn't separate) become dark line pixels.
    px = small.load()
    for y in range(small.size[1]):
        for x in range(small.size[0]):
            r, g, b, al = px[x, y]
            if al and r - g > 50 and b - g > 40 and r > 90:
                px[x, y] = (60, 52, 70, al)
    # Hard alpha.
    a = small.getchannel("A").point(lambda v: 255 if v >= 110 else 0)
    rgb = small.convert("RGB").quantize(colors=colors, method=Image.Quantize.MEDIANCUT).convert("RGB")
    out = Image.merge("RGBA", (*rgb.split(), a))
    # 1 px outline on the outside, with a 1 px margin so it fits.
    W, H = out.size
    canvas = Image.new("RGBA", (W + 2, H + 2), (0, 0, 0, 0))
    canvas.paste(out, (1, 1))
    src = canvas.load()
    res = canvas.copy()
    dst = res.load()
    for y in range(H + 2):
        for x in range(W + 2):
            if src[x, y][3] == 0:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < W + 2 and 0 <= ny < H + 2 and src[nx, ny][3] > 0:
                        dst[x, y] = OUTLINE
                        break
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sheet")
    ap.add_argument("name")
    ap.add_argument("--height", type=int, default=0, help="target sprite height in pixels (art px), per piece")
    ap.add_argument("--scale", type=float, default=0.0, help="one source->art scale for every piece (keeps relative sizes)")
    ap.add_argument("--block", type=int, default=0, help="source is pixel art on a block-px grid: sample block centres")
    ap.add_argument("--boxes", default="", help="explicit crops x0,y0,x1,y1;... (source px) instead of splitting")
    ap.add_argument("--names", default="", help="comma-separated names per piece instead of <name>_<i>")
    ap.add_argument("--out", default="assets/atlas/src")
    ap.add_argument("--colors", type=int, default=16)
    ap.add_argument("--axis", choices=["x", "y"], default="x", help="split objects along columns (x) or rows (y)")
    ap.add_argument("--min-gap", type=int, default=6)
    ap.add_argument("--min-size", type=int, default=40, help="ignore specks smaller than this (source px)")
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    im = key_out(Image.open(a.sheet))
    pieces = []
    if a.boxes:
        for b in a.boxes.split(";"):
            x0, y0, x1, y1 = (int(v) for v in b.split(","))
            pieces.append(im.crop((x0, y0, x1, y1)))
    for s0, s1 in ([] if a.boxes else spans(im, a.axis, a.min_gap)):
        box = (s0, 0, s1, im.size[1]) if a.axis == "x" else (0, s0, im.size[0], s1)
        piece = im.crop(box)
        bb = piece.getbbox()
        if bb and (bb[2] - bb[0]) >= a.min_size // 2 and (bb[3] - bb[1]) >= a.min_size:
            pieces.append(piece)
    if not pieces:
        sys.exit("no objects found: is the background magenta?")
    if not a.height and not a.scale and not a.block:
        sys.exit("give --height, --scale or --block")
    sprites = [pixelate(p, a.height, a.colors, a.scale or None, a.block) for p in pieces]
    names = a.names.split(",") if a.names else [f"{a.name}_{i}" for i in range(len(sprites))]
    for n, sp in zip(names, sprites):
        sp.save(os.path.join(a.out, f"{n}.png"))
    # Contact sheet at 6x on a mid-grey checker so transparency reads.
    z = 6
    W = sum(s.size[0] for s in sprites) + 4 * (len(sprites) + 1)
    H = max(s.size[1] for s in sprites) + 8
    sheet = Image.new("RGBA", (W, H), (70, 70, 80, 255))
    x = 4
    for s in sprites:
        sheet.alpha_composite(s, (x, 4))
        x += s.size[0] + 4
    sheet.resize((W * z, H * z), Image.NEAREST).save(os.path.join(a.out, f"{a.name}_preview.png"))
    print(f"{len(sprites)} sprite(s): " + ", ".join(f"{n} {s.size[0]}x{s.size[1]}" for n, s in zip(names, sprites)))


if __name__ == "__main__":
    main()
