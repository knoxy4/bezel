"""Pack assets/atlas/src/*.png (not *_preview.png) into assets/atlas/hero.png + assets/atlas/hero.atlas.

hero.atlas is one line per sprite, no dependencies needed to read it:
    <name> <x> <y> <w> <h> <pivot_x> <pivot_y>
Pivot is a 0..1 fraction of the frame; default bottom-centre (feet), overridable in assets/atlas/pivots.txt
(`<name> <px> <py>` per line). Frames of one animation are separate sprites named <name>_<n> for now.

    python tools/pack_atlas.py [--src assets/atlas/src] [--out assets/atlas] [--width 512]
"""
import argparse
import os

from PIL import Image


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default="assets/atlas/src")
    ap.add_argument("--out", default="assets/atlas")
    ap.add_argument("--width", type=int, default=512)
    a = ap.parse_args()

    pivots = {}
    pf = os.path.join(a.out, "pivots.txt")
    if os.path.exists(pf):
        for line in open(pf, encoding="utf-8"):
            parts = line.split()
            if len(parts) == 3 and not line.startswith("#"):
                pivots[parts[0]] = (float(parts[1]), float(parts[2]))

    items = []
    for f in sorted(os.listdir(a.src)):
        if f.endswith(".png") and not f.endswith("_preview.png"):
            items.append((f[:-4], Image.open(os.path.join(a.src, f)).convert("RGBA")))
    if not items:
        raise SystemExit("nothing to pack")
    # Every sprite also gets a mirrored copy `<name>~f` (frame 1 in the engine), so flipping needs no transforms.
    items += [(n + "~f", im.transpose(Image.FLIP_LEFT_RIGHT)) for n, im in list(items)]
    # Shelf pack, tallest first, 1 px gutter.
    items.sort(key=lambda t: -t[1].size[1])
    x = y = shelf = 0
    placed = []
    for name, im in items:
        w, h = im.size
        if x + w + 1 > a.width:
            x, y, shelf = 0, y + shelf + 1, 0
        placed.append((name, im, x, y))
        x += w + 1
        shelf = max(shelf, h)
    H = y + shelf
    sheet = Image.new("RGBA", (a.width, H), (0, 0, 0, 0))
    lines = []
    for name, im, px, py in sorted(placed, key=lambda t: t[0]):
        sheet.paste(im, (px, py))
        base = name[:-2] if name.endswith("~f") else name
        pvx, pvy = pivots.get(base, (0.5, 1.0))
        if name.endswith("~f"):
            pvx = 1.0 - pvx
        lines.append(f"{name} {px} {py} {im.size[0]} {im.size[1]} {pvx} {pvy}")
    sheet.save(os.path.join(a.out, "hero.png"))
    open(os.path.join(a.out, "hero.atlas"), "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
    print(f"packed {len(lines)} sprites into {a.width}x{H}")


if __name__ == "__main__":
    main()
