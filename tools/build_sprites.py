"""Gemini sheets (assets/art/gemini/*.jpg) -> named pixel sprites (assets/atlas/src) -> atlas (assets/atlas/hero.*).

Reproducible: every sheet's cut settings and every piece's name live in SPEC below. `None` drops a piece.
`block` is source px per art px for that sheet, chosen so sprites land at game scale against a ~29 px miner (D46;
Gemini doesn't keep one pixel size across sheets). `boxes` overrides detection where objects touch.

    python tools/build_sprites.py
"""
import os
import subprocess
import sys

from PIL import Image

sys.path.insert(0, os.path.dirname(__file__))
from sprite_auto import components, keyed  # noqa: E402
from sprite_cut import pixelate  # noqa: E402

SRC = "assets/art/gemini"
OUT = "assets/atlas/src"


def x2(boxes):
    """Boxes read off the 1376x768 outline previews, scaled to the 2752x1536 source."""
    return [tuple(v * 2 for v in b) for b in boxes]


SPEC = {
    "miners": dict(block=23, names=["miner_idle", "miner_walk_0", "miner_walk_1", "miner_climb", "miner_mine", "miner_carry", "miner_fall", "miner_land"]),
    "parachute": dict(block=18, names=["chute_0", "chute_1", "chute_2", "canopy_0", "canopy_1", "canopy_2"]),
    "outpost": dict(block=13, names=["outpost_full", "outpost_build", "outpost_wreck"]),
    "girder": dict(block=20, names=["girder_seg", "girder_corner_l", "girder_corner_r", "girder_clamp"]),
    "davits": dict(block=20, names=["davit_r", None, "davit_l", None]),
    "foundry": dict(block=15, names=["furnace_0", "furnace_1", "furnace_2", None, None, None, "anvil", "crates", "bucket"]),
    "drill": dict(block=16, names=["cage_empty", "cage_full", "drill_0", "drill_1", "drill_2", None, None]),
    "tiles": dict(block=12, merge=8, names=["tile_earth", None, "tile_rail", "tile_shaft", "tile_pipe", None]),
    "bits": dict(block=22, names=["crystal_s", "crystal_m", "crystal_l", "core", "crate", "debris_0", "plank", "girder_bent", "cog", "sheet_metal"]),
    "fx": dict(block=14, boxes=x2([
        (60, 60, 116, 112), (206, 30, 316, 148), (354, 14, 512, 164), (538, 20, 666, 158),
        (44, 232, 134, 326), (194, 226, 322, 336), (354, 208, 512, 348), (532, 208, 678, 348), (706, 214, 848, 346),
        (56, 438, 122, 508), (190, 392, 332, 546), (348, 392, 516, 558), (544, 408, 672, 538),
        (68, 576, 104, 758), (228, 576, 288, 758), (384, 576, 480, 758), (566, 576, 654, 758),
    ]), names=["spark_0", "spark_1", "spark_2", "spark_3", "dust_0", "dust_1", "dust_2", "dust_3", "dust_4",
               "ping_0", "ping_1", "ping_2", "ping_3", "snap_0", "snap_1", "snap_2", "snap_3"]),
    "terrarium": dict(block=27, boxes=[(30, 340, 660, 1250), (690, 440, 1112, 1250), (1112, 330, 1400, 1250),
                                       (1420, 470, 1850, 1260), (1870, 760, 2300, 1270), (2270, 430, 2720, 1250)],
                      names=["plant_fern", "plant_tentacle", "plant_eye", "plant_mushroom", "plant_spiky", "plant_star"]),
    # Moto spike (2026-11-12): Knox's dirtbike sheets, rider faces right.
    "moto/bike1": dict(block=8, merge=12, names=[
        "ride_0", "ride_1", None, "ride_2", "wheelie_0", "wheelie_1", "ride_3", "air_up", "air_mid", "air_down",
        "land_0", "land_1", "ride_4", "crash_0", "crash_1", "crash_2"]),
    "moto/bike2": dict(block=8, merge=12, names=[
        "trick_nohands", "trick_heel", "trick_cancan", "trick_legover", "trick_nacnac", "wheelie_2",
        "flip_0", "flip_1", "flip_2", "flip_3", "ride_5", "trick_superman_0", "trick_superman_1",
        "air_1", "air_2", "air_3", "air_4", "skid_0", "skid_1", "fist_0", "fist_1", "fist_2"]),
    # Builder crew: Gemini mixed shirt colours across frames, so every frame is recoloured into three crew members.
    "moto/climber": dict(block=11, merge=12, crew=True, names=[
        "hang", "climb_0", "climb_1", "climb_2", "reach_side", "crouch", "jump_0", "jump_1", "reach_0", "reach_1",
        "spread_0", "spread_1", None, "spread_2", "climb_3", "climb_4", "mantle", "fall_0", "fall_1", "fallen",
        "cheer_0", "cheer_1"]),
    # Cast, 2026-11-12. spider.jpg is an earlier take of spider2.jpg, so only spider2 is cut.
    "moto/ninja": dict(block=8, merge=10, names=[
        "ninja_stance", "ninja_run_0", "ninja_run_1", "ninja_run_2", "ninja_run_3", "ninja_dash", "ninja_leap",
        "ninja_tuck", "ninja_flip_0", "ninja_flip_1", "ninja_flip_2", "ninja_stand", "ninja_land", "ninja_cling_0",
        "ninja_cling_1", "ninja_vault", "ninja_air", "ninja_kick", "ninja_aim", "ninja_throw", "ninja_kunai",
        "ninja_kunai_stuck", "ninja_rope", None, "ninja_hit", "ninja_bomb_0", "ninja_bomb_1", "smoke_0", "smoke_1",
        None, None, None, None, None, None, None]),
    "moto/spider2": dict(block=8, merge=10, names=[
        "spider_walk_0", "spider_walk_1", "spider_walk_2", "spider_walk_3", "spider_walk_4", "spider_under_0",
        "spider_walk_5", "spider_under_1", "spider_under_2", "spider_walk_6", "spider_under_3", "spider_walk_7",
        "spider_wall_0", "spider_wall_1", "spider_wall_2", "spider_wall_3", "spider_crouch", "spider_drop",
        "spider_hang_0", "spider_hang_1", "spider_hang_2", "spider_climb_0", "spider_climb_1", "spider_hang_3",
        "spider_top", "spider_creep_0", "spider_creep_1", "spider_creep_2", "spider_shoot", "spider_lasso",
        "spider_pounce", "spider_swing", "spider_dangle", "spider_creep_3", "spider_ball_0", "spider_ball_1",
        "spider_dead"]),
    "moto/bottom": dict(block=8, merge=10, names=[
        "robo_leap", None, "robo_walk_0", "robo_walk_1", "robo_walk_2", "robo_walk_3", None, "robo_dangle",
        "robo_shoot", "robo_walk_4", "robo_ball", "robo_dead", "cham_idle", "cham_walk_0", "cham_walk_1", None, None,
        None, None, None, "cham_tongue_0", "cham_tongue_1", None, "cham_leap", "cham_sleep",
        "grap_run_0", "grap_run_1", "grap_run_2", "grap_run_3", "grap_shoot", "grap_swing_0", "grap_swing_1",
        "grap_swing_2", "grap_climb_0", "grap_climb_1", "grap_hang", "grap_fall"]),
    # Objects touch (beams, sparks), so boxes are hand-set: displayed-preview px x1.2 = source px.
    "moto/ironworker": dict(block=8, colors=40, boxes=[tuple(round(v * 1.2) for v in b) for b in [
        (20, 30, 215, 345), (1025, 30, 1262, 345), (1262, 30, 1495, 345), (1490, 30, 1715, 345), (1715, 30, 1985, 345),
        (40, 365, 205, 650), (270, 365, 430, 650), (505, 365, 670, 650), (730, 365, 890, 650),
        (15, 668, 250, 940), (270, 668, 470, 940), (520, 690, 790, 940), (800, 685, 1100, 940),
        (470, 965, 680, 1195), (705, 965, 940, 1195), (955, 965, 1280, 1195),
        (30, 1270, 280, 1460), (290, 1270, 550, 1460), (1300, 1265, 1520, 1460), (1755, 1205, 1980, 1460),
    ]], names=[
        "iron_stand", "iron_carry_0", "iron_carry_1", "iron_carry_2", "iron_carry_3",
        "iron_climb_0", "iron_climb_1", "iron_climb_2", "iron_climb_3",
        "iron_sledge_0", "iron_sledge_1", "iron_sledge_2", "iron_sledge_3",
        "iron_weld_0", "iron_weld_1", "iron_weld_2", "iron_fall_0", "iron_fall_1", "iron_kneel", "iron_flex"]),
}

# 3D-render sheets on white, 5x5 grids (Knox, 2026-11-12): kept big and detailed, not pixel-grid sampled.
# (sheet, cells to keep in order, sprite name prefix, height in art px)
# Walk cells 2, 7, 8, 12, 13, 18 and 23 are render glitches (the rear ankle vanishes as the feet pass); the other
# 18 are used in order. The 25 cells are ~5 steps, not one cycle (moto.rs GIANT_CYCLE/GIANT_STEPS match that).
# One scale per sheet (tallest frame = height) and one crop per sheet: the idle sheet was rendered smaller, and
# per-frame fitting made him shrink when he stopped and jitter while walking.
GRID = [
    ("moto/walker_walk.png", [i for i in range(25) if i not in (2, 7, 8, 12, 13, 18, 23)], "giant_walk_", 200),
    ("moto/walker_idle.png", [0], "giant_idle_", 200),
]


def unwhite(im):
    """Flood-fill white background in from the edges, so white socks and stripes inside the body survive."""
    from collections import deque
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    seen = bytearray(w * h)
    q = deque([(x, y) for x in range(w) for y in (0, h - 1)] + [(x, y) for y in range(h) for x in (0, w - 1)])
    while q:
        x, y = q.popleft()
        if seen[y * w + x]:
            continue
        seen[y * w + x] = 1
        if min(px[x, y][:3]) < 235:
            continue
        px[x, y] = (0, 0, 0, 0)
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx]:
                q.append((nx, ny))
    return im


def big_sprite(cell, scale):
    """Downscale a rendered figure (already cropped) by `scale`, punch up contrast to sit next to the flat pixel art,
    quantize and add the atlas's 1 px dark outline."""
    from PIL import ImageEnhance
    small = cell.resize((max(1, round(cell.size[0] * scale)), max(1, round(cell.size[1] * scale))), Image.LANCZOS)
    a = small.getchannel("A").point(lambda v: 255 if v > 110 else 0)
    rgb = small.convert("RGB")
    rgb = ImageEnhance.Brightness(rgb).enhance(1.15)
    rgb = ImageEnhance.Contrast(rgb).enhance(1.2)
    rgb = ImageEnhance.Color(rgb).enhance(1.3)
    rgb = rgb.quantize(40, method=Image.Quantize.MEDIANCUT).convert("RGB")
    sp = Image.merge("RGBA", (*rgb.split(), a))
    w, h = sp.size
    o = Image.new("RGBA", (w + 2, h + 2), (0, 0, 0, 0))
    o.paste(sp, (1, 1))
    p, q = o.load(), sp.load()
    for y in range(h + 2):
        for x in range(w + 2):
            if p[x, y][3]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                sx, sy = x - 1 + dx, y - 1 + dy
                if 0 <= sx < w and 0 <= sy < h and q[sx, sy][3]:
                    p[x, y] = (22, 18, 26, 255)
                    break
    return o


# Crew shirt hues (degrees): a = teal, b = orange, c = green.
CREW = {"a": 185, "b": 28, "c": 112}


def shirt(h, s, v):
    """Shirt pixels in the climber sheet: teal, green, or saturated orange (skin is orange too, but much paler)."""
    return (160 <= h <= 205 and s > 0.35) or (75 <= h <= 145 and s > 0.35) or (12 <= h <= 45 and s > 0.72 and v > 0.55)


def recolor(sp, hue):
    import colorsys
    out = sp.copy()
    px = out.load()
    for y in range(out.size[1]):
        for x in range(out.size[0]):
            r, g, b, a = px[x, y]
            if not a:
                continue
            h, s, v = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
            if shirt(h * 360, s, v):
                s2 = max(s, 0.55)
                r2, g2, b2 = colorsys.hsv_to_rgb(hue / 360, s2, min(v, 0.72))
                px[x, y] = (int(r2 * 255), int(g2 * 255), int(b2 * 255), a)
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        if f.endswith(".png"):
            os.remove(os.path.join(OUT, f))
    total = 0
    for sheet, spec in SPEC.items():
        im = keyed(Image.open(os.path.join(SRC, sheet + ".jpg")))
        boxes = spec.get("boxes") or [tuple(b) for b in components(im, 4, spec.get("merge", 6))]
        names = spec["names"]
        if len(boxes) != len(names):
            sys.exit(f"{sheet}: found {len(boxes)} pieces but SPEC names {len(names)}; re-check the outline preview")
        made = []
        for box, name in zip(boxes, names):
            if name is None:
                continue
            sp = pixelate(im.crop(box), 0, spec.get("colors", 24), None, spec["block"])
            if spec.get("crew"):
                for k, hue in CREW.items():
                    recolor(sp, hue).save(os.path.join(OUT, f"crew{k}_{name}.png"))
            else:
                sp.save(os.path.join(OUT, name + ".png"))
            made.append(f"{name} {sp.size[0]}x{sp.size[1]}")
        total += len(made)
        print(f"{sheet}: " + ", ".join(made))
    for sheet, cells, prefix, height in GRID:
        im = unwhite(Image.open(os.path.join(SRC, sheet)))
        cw, ch = im.size[0] // 5, im.size[1] // 5
        cs = [im.crop(((i % 5) * cw, (i // 5) * ch, (i % 5 + 1) * cw, (i // 5 + 1) * ch)) for i in cells]
        boxes = [c.getchannel("A").point(lambda v: 255 if v > 110 else 0).getbbox() for c in cs]
        union = (min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes))
        scale = height / max(b[3] - b[1] for b in boxes)
        for n, c in enumerate(cs):
            big_sprite(c.crop(union), scale).save(os.path.join(OUT, f"{prefix}{n:02d}.png"))
        total += len(cells)
        print(f"{sheet}: {len(cells)} frames at {height} px")
    print(f"{total} sprites")
    subprocess.run([sys.executable, os.path.join(os.path.dirname(__file__), "pack_atlas.py"), "--width", "1024"], check=True)


if __name__ == "__main__":
    main()
