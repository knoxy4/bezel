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
}


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
            sp = pixelate(im.crop(box), 0, 24, None, spec["block"])
            sp.save(os.path.join(OUT, name + ".png"))
            made.append(f"{name} {sp.size[0]}x{sp.size[1]}")
        total += len(made)
        print(f"{sheet}: " + ", ".join(made))
    print(f"{total} sprites")
    subprocess.run([sys.executable, os.path.join(os.path.dirname(__file__), "pack_atlas.py"), "--width", "1024"], check=True)


if __name__ == "__main__":
    main()
