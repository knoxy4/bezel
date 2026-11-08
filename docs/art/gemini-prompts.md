# Hero sprite prompts for Gemini (Nano Banana) — D46

Version 2 (2026-11-08): no reference images, descriptions only (Knox).

Knox generates in Gemini; Spec cuts and packs. Written 2026-11-08.

## How to run them
1. **No reference images:** every prompt describes the style and the miner in full. Copy one block, paste it into Gemini, done.
2. **Do the miner sheet first.** Once you like a miner, you can say *"use this same miner"* in that chat for the parachute and elevator prompts, but the prompts work without it.
3. If a result drifts (smooth shading, blurry pixels, a gradient background), reply in the same chat:
   *"Same image, but strict pixel art: hard square pixels, no anti-aliasing, flat solid #FF00FF background."*
4. **Save the keepers** as PNG into `assets/art/gemini/` with the file name in each heading, and tell Spec. Spec cuts,
   pixel-snaps and packs them (`tools/sprite_cut.py`, `tools/pack_atlas.py`).

The house rules every prompt ends with (already inside each block):
> strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire
> with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading
> with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan
> accents. One miner is about 32 pixels tall, shown scaled up 8× so every art pixel is a crisp 8×8 block. Side view.
> Solid flat pure magenta background (#FF00FF), no gradient, no ground, no shadow. No text, no labels, no border.

---

## 1. Miner sheet → `miners.png`
```text
Make a sprite sheet of ONE small miner character, the same character in every pose: yellow hard hat with a bright headlamp, orange work overalls, dark grey shirt, brown boots. Two rows of four poses, evenly spaced on a grid. Top row: standing idle; walking step A; walking step B; climbing a vertical rope hand over hand (rope only where the hands grip). Bottom row: swinging a pickaxe at a rock; carrying a glowing cyan crystal on one shoulder; falling with arms and legs out; landing in a crouch. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 2. Parachute → `parachute.png`
```text
Draw this: a small miner character in a yellow hard hat with a bright headlamp, orange work overalls, a dark grey shirt and brown boots hanging under a small round teal parachute with four thin dark lines to the harness, in three frames side by side: swaying left, centred, swaying right. Then, to the right, the parachute canopy alone (no miner) in the same three sway frames. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 3. Outpost on a girder → `outpost.png`
```text
Draw this: A small mining outpost that sits on top of a horizontal steel girder beam: a wooden shack with a lit window and a smoking chimney, a short yellow crane arm, a drill mast with a lamp, a small heap of glowing cyan crystals, two crates. The girder is long and plain so it can be stretched. Draw three versions left to right: the full outpost; the outpost half-built with scaffolding; the outpost wrecked and broken in pieces. About 4 miners wide. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 4. Girder frame + corner pulleys → `girder.png`
```text
Draw this: Parts for a riveted steel frame that runs along the top edge of an app window, with cables hanging from pulleys at its corners: a straight riveted girder segment that tiles seamlessly left to right; a left corner bracket with a round pulley wheel; a right corner bracket with a round pulley wheel; a small cable clamp. Separate pieces in a row. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 5. Davits → `davits.png`
```text
Draw this: Small steel crane davits: an upright post, a horizontal arm reaching out, a pulley wheel and a hook at the arm tip, about one and a half miners tall. Two facing right, two facing left (mirror), in a row. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 6. Foundry → `foundry.png`
```text
Draw this: A brick foundry furnace with an arched mouth and a chimney, about 3 miners wide and 2 miners tall, in three frames side by side where the fire in the mouth flickers (low, medium, roaring orange). To the right: an anvil, a stack of crates, and a bucket of molten metal as separate pieces. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 7. Drill + elevator → `drill.png`
```text
Draw this: Parts for a vertical mine shaft: a big spiral steel drill bit pointing UP under a riveted housing with a lamp (about 4 miners wide, 3 tall), in three frames where the spiral turns; an elevator cage with a lamp, empty, about 2 miners tall; the same cage with two miners inside, each in a yellow hard hat with a bright headlamp and orange work overalls. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 8. Tiles → `tiles.png`
```text
Draw this: Seamless tiling strips, each shown as a 4-repeat strip so the seams can be checked: (a) a trench floor of packed earth with roots and grass tufts, tiling left to right; (b) a rail track with a conduit pipe above it, tiling left to right; (c) a rough stone mine-shaft wall, tiling top to bottom; (d) a vertical steel pipe, tiling top to bottom. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 9. Pickups and debris → `bits.png`
```text
Draw this: A row of small separate items: three glowing cyan crystal shards (small, medium, large); a round glowing cyan power core in a metal ring; a scrap crate; four pieces of wreck debris (broken plank, bent girder, cog, torn sheet metal). House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 10. Effects → `fx.png`
```text
Draw this: Small pixel-art effect frames in rows: sparks bursting (4 frames, yellow-white); a dust puff (4 frames, grey-brown); a cyan glow ping (3 frames); a rope snap whip (3 frames). House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

## 11. Terrarium plants → `terrarium.png`
```text
Draw this: Six separate glowing alien cave plants for an underground terrarium: a teal crystal fern, a purple tentacle bloom, a green eye-pod on a stalk, a cyan mushroom cluster, a pink spiky bulb, a blue star flower. Each about one miner tall. House rules: strict 16-bit pixel art for a cozy night-time mining game, a dark slate-blue world lit by warm amber lamps and fire with glowing cyan crystals: hard square pixels, no anti-aliasing, no blur, chunky 1-pixel dark outlines, flat shading with two tones per material. Palette: slate blue, charcoal grey, rusty brown, warm amber and orange, bright cyan accents. Draw it at a native size where one miner is about 32 pixels tall, then show it scaled up 8× so every art pixel is a crisp 8×8 block. Side view. Solid flat pure magenta background (#FF00FF) with no gradient, no ground, no shadow. Every object separate with clear magenta space around it. No text, no labels, no border.
```

---

## Capsule art (store page), for later
```text
A miniature night-time computer desktop seen up close. One app window with a thin cyan-lit frame has a steel girder along its top edge with pulley wheels at both corners; cyan cables run from the pulleys down both sides to a glowing brick foundry strip along the bottom. A small mining outpost with a smoking chimney sits on the girder; tiny pixel miners climb the cables and float down under teal parachutes holding cyan crystals. A mouse cursor drags a second window through another outpost, smashing it into sparks and debris. Left 45% of the frame is calm dark desktop for a logo. Crisp pixel-art characters, smooth glowing cables and effects. Wide 16:9.
```

(Full capsule set and sizes: `docs/marketing/capsule-prompts.md`.)
