# Capsule art prompts (THOR pipeline)

**Status:** ready for the week-2 style frame. Written 2026-11-07. Nothing has been generated yet.
**Pipeline:** `imagegen` on THOR (FLUX.2-klein-4B, see the thor-imagegen skill). D28: every run takes a **gpu lease**.

Tags: **[VERIFY]** = check before relying on it.

---

## Ground rules

1. **The style frame leads.** Every prompt below runs with `--ref <style-frame.png>` once Knox picks the frame in week 2. Before then, run them without `--ref` only to explore composition.
2. **No text in generated art.** klein renders lettering unreliably. Generate each capsule with the logo area left empty, and lay the BEZEL logotype on top by hand, from one vector source.
3. **Batch, don't loop.** Use `--count 8` per prompt. Keep the seed of every pick, so a final can be regenerated at another size.
4. **Generate at klein's sizes, crop to Steam's.** klein works best near 1 MP. Generate at the ratio below, then upscale and crop to the exact capsule size.
5. **Look:** hybrid art (D8): crisp pixel-art miners and structures, smooth antialiased ropes, glow and effects. The tone is spectacle (D9).

### Steam capsule sizes [VERIFY in Steamworks → Store Assets before cropping]

| Asset | Size (px) | Generate at | Logo safe area |
|---|---|---|---|
| Header capsule | 920 × 430 | 1536 × 720 | left 45% |
| Small capsule | 462 × 174 | 1536 × 576 | logo only; art is background |
| Main capsule | 1232 × 706 | 1536 × 880 | upper left third |
| Vertical capsule | 748 × 896 | 896 × 1072 | top 30% |
| Library capsule | 600 × 900 | 896 × 1344 | top 30% |
| Library hero | 3840 × 1240 | 1536 × 496, then upscale | none (Steam overlays the logo) |
| Library logo | 1280 × 720, transparent | vector only, not generated | — |

---

## Running a batch

Check the board first. The gpu lease covers the run, and the job releases it when done:

```powershell
& C:\Users\KNX\CAGED\tools\thor_lease.ps1 status
& C:\Users\KNX\CAGED\tools\thor_lease.ps1 submit gpu -By bezel-art -For 'BEZEL capsule: header x8' -EstMin 4 -Exe X:\imagegen\.venv\Scripts\python.exe -ArgList 'X:\imagegen\src\imagegen\cli.py --json --style illustration --size 1536x720 --count 8 --ref X:\BEZEL\art\style-frame.png "<prompt>"'
& C:\Users\KNX\CAGED\tools\thor_lease.ps1 job -JobId <id> -Json
```

Estimate ~14 s per image, so 8 images are about 2 minutes, plus a cold start. `X:\BEZEL\art\style-frame.png` is a placeholder path until the frame exists.

---

## Prompts

Style: `illustration` for the capsules. Use `concept` if the illustration preset flattens the depth too much. Prose only; the preset supplies the craft vocabulary.

### 1. Header capsule: the crush (hero shot)

> A cozy dark computer desktop seen up close, like a miniature landscape. Three overlapping application windows stand like cliffs with thin glowing frames. On the top edge of the nearest window, a tiny pixel-art mining outpost with a crane and a drill rig. Smooth glowing ropes hang down the window's side, and small pixel miners in hard hats with headlamps climb them. From the right, a giant translucent mouse cursor drags a window sideways through a second outpost, smashing it into a burst of pixel debris, sparks and scrap. Miners leap away under tiny parachutes. Along the bottom edge runs a warm glowing foundry strip of brick and molten metal. Left 45% of the frame is calm dark desktop, empty, for a logo.

Variant to batch alongside: same scene with the crush happening on the left and the logo space on the right, in case the logotype reads better there.

### 2. Main capsule: the whole world

> A wide view of a computer desktop turned into a mining world. On the left edge, a tall narrow shaft cut into dark rock, with a drill boring upward through glowing strata and monster eyes in the dark. Along the bottom edge, a long trench of warm brick furnaces and conveyor belts, with tiny pixel miners hauling scrap. In the middle, bare desktop space over a soft wallpaper gradient, with application windows floating like mesas. Rope lines and a small outpost cling to the top edge of the nearest window. One window is mid-drag, with a motion smear and a scatter of debris behind it. Upper left third is open sky for a logo.

### 3. Vertical and library capsules: the climb

> A tall view up the side of a single application window standing like a skyscraper cliff. Tiny pixel-art miners in hard hats climb glowing ropes up its frame toward a mining rig perched on the top edge, lit by headlamps and the glow of a foundry far below. Scrap and a falling glowing core drop past them toward the bottom of the frame. Top 30% is calm dark space for a logo.

### 4. Small capsule: background only

> Dark computer-desktop texture with a thin warm glow along the bottom edge and the faint silhouette of a rope and a tiny pixel miner hanging at the far right. Very simple, low detail, for a small banner behind a logo.

### 5. Library hero: the strip

> An extremely wide panoramic strip of a computer desktop as a mining frontier at night. Floating application windows like mesas, rope lines and zip lines strung between their top edges, small pixel-art outposts with lights, a foundry glow along the bottom, a drill shaft at the far left. Calm, atmospheric, no central subject, so a logo can sit anywhere.

---

## Review checklist (Knox, week 2)

- [ ] Miners read as pixel art; ropes and effects read as smooth (the D8 hybrid).
- [ ] Nothing in the art suggests features after the fest as if they were in the build. Monster eyes in the main capsule are atmosphere; drop them if they read as a promise.
- [ ] Windows look like generic app windows: no real app's UI, logo or trademark.
- [ ] Logo areas are clean.
- [ ] Seeds of every pick are recorded next to the files.
- [ ] Steam's AI-content disclosure is filled in before the page goes up (store-copy-v1.md).
