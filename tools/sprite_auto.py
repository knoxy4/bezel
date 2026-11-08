"""Auto-cut a Gemini sprite sheet (objects on flat magenta, drawn ~8x up) into numbered pixel-art sprites.

Key out magenta -> find objects as connected regions (nearby parts merged) -> reading order -> sample each object's
pixel grid at block centres -> palette + hard alpha + 1 px outline. Writes <out>/<name>_<nn>.png and a numbered 3x
contact sheet <out>/<name>_preview.png. Map numbers to sprite names afterwards with tools/sprite_names.txt.

    python tools/sprite_auto.py <sheet> <name> [--block 8] [--out assets/atlas/raw] [--merge 24] [--colors 24]
"""
import argparse
import os
import sys
from collections import deque

from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(__file__))
from sprite_cut import pixelate  # noqa: E402


def keyed(im):
    """RGBA with magenta (and JPEG fringe around it) made transparent."""
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, _ = px[x, y]
            if r > 150 and b > 150 and g < min(r, b) - 70:
                px[x, y] = (0, 0, 0, 0)
    return im


def components(im, step, merge):
    """Bounding boxes (source px) of opaque regions, found on a step-downsampled mask; boxes within `merge` px join."""
    w, h = im.size
    a = im.getchannel("A")
    W, H = w // step, h // step
    small = a.resize((W, H), Image.BOX).load()
    seen = bytearray(W * H)
    boxes = []
    for y in range(H):
        for x in range(W):
            if seen[y * W + x] or small[x, y] < 40:
                continue
            q = deque([(x, y)])
            seen[y * W + x] = 1
            x0 = x1 = x
            y0 = y1 = y
            n = 0
            while q:
                cx, cy = q.popleft()
                n += 1
                x0, x1, y0, y1 = min(x0, cx), max(x1, cx), min(y0, cy), max(y1, cy)
                for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                    if 0 <= nx < W and 0 <= ny < H and not seen[ny * W + nx] and small[nx, ny] >= 40:
                        seen[ny * W + nx] = 1
                        q.append((nx, ny))
            if n >= 6:
                boxes.append([x0 * step, y0 * step, (x1 + 1) * step, (y1 + 1) * step])
    # Merge boxes that touch or sit within `merge` px (a canopy and its lines, a shard and its glow).
    changed = True
    while changed:
        changed = False
        for i in range(len(boxes)):
            for j in range(i + 1, len(boxes)):
                A, B = boxes[i], boxes[j]
                if A[0] - merge <= B[2] and B[0] - merge <= A[2] and A[1] - merge <= B[3] and B[1] - merge <= A[3]:
                    boxes[i] = [min(A[0], B[0]), min(A[1], B[1]), max(A[2], B[2]), max(A[3], B[3])]
                    boxes.pop(j)
                    changed = True
                    break
            if changed:
                break
    # Reading order: rows by vertical centre (rows ~ 1/3 of the tallest box apart), then left to right.
    tall = max((b[3] - b[1]) for b in boxes) if boxes else 1
    boxes.sort(key=lambda b: (round(((b[1] + b[3]) / 2) / (tall * 0.8)), b[0]))
    return boxes


def find_block(piece, lo=8, hi=28):
    """Best (block, phase_x, phase_y) for a pixel-art piece drawn n-x up: rebuild it from block-centre samples and
    keep the grid with the lowest error over opaque pixels."""
    px = piece.load()
    w, h = piece.size
    best = None
    for b in range(lo, hi + 1):
        for ph in range(0, b, max(1, b // 4)):
            err = n = 0
            for y in range(ph, h - b, b):
                for x in range(ph, w - b, b):
                    c = px[x + b // 2, y + b // 2]
                    if c[3] == 0:
                        continue
                    for dy in (1, b - 2):
                        for dx in (1, b - 2):
                            d = px[x + dx, y + dy]
                            if d[3]:
                                err += abs(d[0] - c[0]) + abs(d[1] - c[1]) + abs(d[2] - c[2])
                                n += 1
            if n > 200:
                score = err / n
                if best is None or score < best[0]:
                    best = (score, b, ph)
    return best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sheet")
    ap.add_argument("name")
    ap.add_argument("--block", type=int, default=0, help="0 = find it from the largest object")
    ap.add_argument("--out", default="assets/atlas/raw")
    ap.add_argument("--merge", type=int, default=24)
    ap.add_argument("--colors", type=int, default=24)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    im = keyed(Image.open(a.sheet))
    boxes = components(im, 4, a.merge)
    block, phase = a.block, 0
    if not block:
        big = max(boxes, key=lambda b: (b[2] - b[0]) * (b[3] - b[1]))
        score, block, phase = find_block(im.crop(tuple(big)))
        print(f"{a.name}: block {block} (phase {phase}, err {score:.1f})")
    sprites = []
    for i, b in enumerate(boxes):
        sp = pixelate(im.crop(tuple(b)), 0, a.colors, None, block)
        sp.save(os.path.join(a.out, f"{a.name}_{i:02d}.png"))
        sprites.append(sp)
    z = 3
    pad = 14
    W = min(2400, sum(s.size[0] * z + pad for s in sprites) + pad)
    rows, x, y, rowh = [], pad, 18, 0
    placed = []
    for i, s in enumerate(sprites):
        sw, sh = s.size[0] * z, s.size[1] * z
        if x + sw + pad > W:
            x, y, rowh = pad, y + rowh + 22, 0
        placed.append((i, s, x, y))
        x += sw + pad
        rowh = max(rowh, sh)
    sheet = Image.new("RGBA", (W, y + rowh + pad), (70, 70, 80, 255))
    d = ImageDraw.Draw(sheet)
    for i, s, px, py in placed:
        sheet.alpha_composite(s.resize((s.size[0] * z, s.size[1] * z), Image.NEAREST), (px, py))
        d.text((px, py - 14), f"{i:02d} {s.size[0]}x{s.size[1]}", fill=(255, 255, 0, 255))
    sheet.save(os.path.join(a.out, f"{a.name}_preview.png"))
    print(f"{a.name}: {len(sprites)} objects: " + " ".join(f"{i:02d}={s.size[0]}x{s.size[1]}" for i, s in enumerate(sprites)))


if __name__ == "__main__":
    main()
