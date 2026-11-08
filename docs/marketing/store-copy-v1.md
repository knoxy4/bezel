# BEZEL store copy, draft v1

**Status:** draft for Knox's review in week 2. Written 2026-11-07.
**Page type:** Coming Soon, release date "To be announced" (D40). No price shown yet (D29: premium, one-time).
**Rule for this page:** describe only what the fest cut will show (fest plan §2). Anything after the fest is labeled as planned.

Tags: **[VERIFY]** = check against Steamworks before publishing.

---

## Short description

Steam caps this at about 300 characters [VERIFY]. Pick one.

**A (lead with the catastrophe):**
> Tiny miners move into the edges of your Windows desktop. They grapple onto your windows, string ropes through the gaps and haul scrap home to a foundry that runs on your typing. Then you drag a window across their work and wreck it. A desktop companion that builds while you work.

**B (lead with the companion):**
> A mining colony that lives on your desktop while you work. Miners hook onto your windows, build in the bare gaps between them and carry salvage home along the edges of your screen. Your typing heats their foundry. Your window drags are their earthquakes.

---

## Long description

### Your desktop is the map

BEZEL turns the edges and empty spaces of your Windows desktop into a living mine.

- **Bedrock.** Two thin bars dock to the left and bottom of your main monitor, like the taskbar does. The bottom bar is the Trench, home to the foundry. The left bar is the Shaft, where the drill bores upward. Maximized windows fit beside them, so they never cover your work.
- **Frontier.** Every patch of bare desktop, on every monitor, becomes open ground. BEZEL draws it behind all of your windows, on top of your own wallpaper. Your desktop icons stay visible and clickable.

### Windows are terrain

Miners grapple onto the window you're using, set up a rig on its top edge and run rope lines down its sides. The top edge of any window becomes a platform they can land on.

### You are the catastrophe

Drag a window across their work and you crush it. Rigs break, ropes snap and miners parachute to safety. Part of what you wrecked comes back as salvage, which they carry home. Anything a window merely covers (opening an app, maximizing, switching virtual desktops) gets buried, and the miners dig it out once it's uncovered.

### Your work powers the mine

BEZEL counts how active your keyboard and mouse are. It never records which keys you press. That activity heats the foundry, which smelts scrap into fuel. The fuel drives the drill up the Shaft, and every so often a core falls home.

### Built to stay out of your way

- It runs all the time, alongside your work.
- It never takes focus and never covers a scrollbar.
- It hides when a game or video goes fullscreen.
- It works alongside live wallpaper apps by drawing the frontier over them. [VERIFY before publishing: tested with one live wallpaper app on Windows 11 so far.]

### In development

This is a Coming Soon page. The first public build focuses on the core loop: grappling, ropes and platforms, crushing and burying, salvage, the foundry and the drill.

**Planned after that** (not in the first build):
- monsters breaking out of the Shaft onto your desktop, with automatic turrets to fight them
- refineries and scrap caches
- zip lines between windows
- a full battle in the Shaft
- optional sound (BEZEL is silent by default)

---

## Privacy (plain words)

Use this block on the store page and in the first-run card, word for word where possible.

> **What BEZEL sees, and what it doesn't**
> - **Activity counts, never keys.** BEZEL counts how many keys you press and how much you move and click the mouse. It never knows which keys. Turning this off takes one click, in the first-run card or the tray menu.
> - **Window positions, never contents.** BEZEL sees where your windows are and how big they are. It never reads their titles, their contents or which programs they belong to.
> - **Your wallpaper is never changed.** BEZEL reads your wallpaper so it can draw on top of it. It never writes it, and when BEZEL closes, your desktop is exactly as it was.
> - **Nothing leaves your PC.** BEZEL sends none of this anywhere. [VERIFY: Steam Cloud saves, if added, hold game progress only; update this line when Steamworks is wired in.]
> - **See it yourself.** A panel in settings shows exactly what BEZEL sees, live: the activity counts, the window rectangles and the wallpaper it's reading.

Engineering source for each claim: Raw Input header-only counts (ADR §5.6), window rects and ids only (ADR §5.4, CLAUDE.md trust rules), invariant 6 (ADR §2.7), the D36 see-panel. If the code ever stops matching a claim, the claim comes off the page the same day.

---

## AI-generated content disclosure (D28)

Steam requires a disclosure for AI-generated content [VERIFY current form fields and wording]. Draft answer:

> BEZEL's artwork (sprites, effects and store art) is produced with AI image generation run locally by the developers, then edited and assembled by hand. The game does not generate AI content while you play.

---

## Tags (truthful and on theme)

Candidates [VERIFY against Steam's current tag list and the fest's theme]: Desktop Companion (or the closest current tag), Idle, Automation, Base Building, Pixel Graphics, Casual, Singleplayer, Relaxing.

Leave off for now: anything about combat or monsters, until breaches ship.

---

## System requirements

- **OS:** Windows 11 (64-bit). Windows 10 is not supported (D11).
- **Processor / memory / graphics:** to be filled in after spike S4 measures the real footprint [VERIFY]. Target: light enough to leave running all day.
- **Notes:** uses the desktop's edges and the space behind your windows. Works with multiple monitors.

---

## Open questions for Knox

1. Short description A or B?
2. "You are the catastrophe" sets the tone. Too much, or right?
3. Should the page name live wallpaper apps (Wallpaper Engine, Lively) explicitly? Naming them helps search, but it needs their compatibility verified first.
