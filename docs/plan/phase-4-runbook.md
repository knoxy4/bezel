# Phase 4 runbook (was "W4"; D44)

**Gate: trailer-ready** (fest plan §4). If it passes, Knox pays the Steam Direct fee (D39).
- The loop runs on THOR's real desktop: miners hook a window, a rig and ropes go up, a drag crushes and salvage comes
  home, typing heats the foundry.
- Hero sprites come from the locked style frame.
- 30 seconds of footage could be captured that day without anyone being embarrassed.

**Started:** 2026-11-07, straight after the phase-3 gate.

## Order (Spec's call; Knox said start)
1. **Wall-run first.** Phase-3 runs proved frontier mode rarely fires on a real layout (windows hug the top edge or are
   maximized). Wall-run (§2.3): a target maximized or snapped flush against SHAFT gets miners climbing SHAFT's inner wall
   to hook its top-left corner, mining it, and sliding home. Drawn inside SHAFT only: nothing paints over the window.
2. **Shaft lite** (fest §2): the drill burns fuel and bores up; a core drops down the shaft on a timer and lands in the trench.
3. **Crush effects + readable delivery:** cut ropes whip and fall instead of vanishing, sparks at the wreck, a hatch
   glow in the trench where a miner comes home.
4. **Hero sprites + atlas** (D28): sprite-forge renders + FLUX paintover with `--ref assets/art/style-frame.png`; the
   atlas loader in render-d2d; Knox approves the hero set.

## Carried over
- First look at the davit build.
- `tools/run-bezel.ps1`: the lease-safe way to play (thor_lease `run`), replacing take/run/release by hand.
- Re-run the style close-up batch on a quiet THOR with the frames-are-terrain rule.
- Open design question: canopy for a normal window hugging the top edge (band is a stretch; a side-hung rig is a cheaper option). Decide after wall-run.

## Knox
- Approve the hero sprite set. Final capsule picks. Choose tags. Answer the three store-copy questions.

## Status (2026-11-08)
- `tools/run-bezel.ps1` written (thor_lease `run`: the lease lives as long as the app).
- Wall-run (sim + SHAFT drawing): a maximized or flush-left target the frontier can't use gets a 3-miner crew that climbs SHAFT's inner wall, hooks the window's top-left corner, mines and slides home; target change releases it, moving the window snaps it (with the rebuild cooldown). Nothing is drawn over the window.
- Shaft lite: the drill burns 1 fuel per 1.5 s and bores 1 m; every 40 s of drilling a core drops down the shaft into the trench (+15 scrap, `cores` in the ledger). Strata scroll with depth; a gauge shows the next core.
- Effects: cut ropes whip and fall for 3 s, then retire; 18 sparks at a wreck; trench hatches glow where cargo lands. Ropes are cyan (inspiration 01).
- Tests: physics 8, layout 10, sim 15 (new: wall-run hauls scrap and stays inside SHAFT, wall-run snap, drill drops a core, crush effects fade). Not run on the desktop yet.
- Art reference: `assets/art/reference/inspiration-01.jpg` + `docs/art/inspiration.md`.
- **D45 (Knox, 2026-11-08): a rig rides its own window.** Dragging the anchor carries the rig, davits and rope tops (ropes swing); another window's drag still crushes; closing collapses it; minimize/hide/virtual-desktop buries it until the window returns. Wall-run still snaps. Sim tests: 16.
- Run 1 of phase 4 (2026-11-08, `bezel.log`): wall-run hooked two windows and snapped when they moved; 3 cores landed (drill to 107 m); rigs deployed with 2 ropes each. **Bug:** rigs 8, 12 and 16 were crushed on the first few pixels of dragging their own window, despite D45 passing its tests. Crush events now log their cause (`Swept`/`DragCover` + the window, or `AnchorClosed`) next to the rig's own window id; next run names the culprit.
- Run 2 of phase 4: Knox, "it's good"; no crush on dragging a rig's own window this time (the run-1 crushes didn't reproduce; crush-cause logging stays in). Knox: ropes should **bounce off the window, not swing behind it** → each rope now treats its anchor window as a solid collider (nodes pushed out along the shallowest axis, sideways speed reflected at 0.35); the collider follows the window under D45. Ropes spawn swung outward. Physics tests: 9.

