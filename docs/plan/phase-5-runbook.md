# Phase 5 runbook: dirtbikes (D48)

**Gate:** the fest build. Finished and polished by about **Dec 20** (D47); Knox pays the Steam fee then.

## Spike result (2026-11-12)
- `bezel-app --moto` (`tools\run-bezel.ps1 -Moto`): 3 riders on a course across all 3 monitors. Floors per monitor (TRENCH floor on home), window tops as ledges with kickers at their ends, 7 tricks sized to predicted airtime (12% greedy → "CASED IT"), "ATE IT" when the ridden window moves, typing throttle.
- First run on THOR: 4 window tops used, 19 tricks, 10 crashes (1 ate it), 6 laps, best line 1550. Knox: "that's fucking cool".
- Code: `crates/app/src/moto.rs` (3 tests) + `moto_main` in `crates/app/src/main.rs`. Sprites: 37 bike frames from `assets/art/gemini/moto/bike1.jpg` + `bike2.jpg` via `tools/build_sprites.py`.

## Tasks, in order
1. **Builder crew.** Climbers scale the window side, hammer, and the kicker appears (climber sheet). Kickers become built objects with a build time, not free.
2. **Skater** as a second rider class: grinds window tops like rails (skater sheet).
3. **Bar track on every monitor** so laps never vanish behind maximized windows (needs per-monitor bar surfaces: a `SurfaceId` change and an ADR §8 update).
4. **Save + parts shop on the bar** (engine, suspension, tires, kicker kits); the panel's save/feedback findings carry over.
5. **Tuning pass:** speed, gravity, kicker pop, trick odds, combo scoring.
6. **Canon:** rewrite ADR §2 game sections and §8 sim signatures for D48; move moto out of the app crate into the sim layer (`core ← sim`, no `windows`), so `tools/check-deps.ps1` covers it.

## Status (2026-11-12, after the spike)
- **Build crew:** ironworkers only (Knox: a full crew at all times), 5, or 10 under chaos; two per kicker; they reinforce finished kickers with hammering and sparks, and haul beams along the floors when there's nothing to build. Kickers are built objects.
- **Cast:** a giant walker on the floors (3D render sheets, glitched frames dropped); ninjas (launch off every edge, flips, wall-climb taller windows, smoke-bomb out on a crash); rope-wrist men (grapple a window bottom mid-air, swing, let go); black widows dropping from window bottoms (riders who hit one crash); chameleons on window tops (camouflage in and out). Robo-spider sprites are cut but unused.
- **Scale:** everyone but the giant at 1.6× (`CAST`); the cast is multiplied by `CHAOS` (x10 was too much, now 3).
- **No bottom bar** in moto mode (Knox): nothing docks; every monitor's floor is its work-area bottom. Score shows as pop-ups and in `bezel.log`.
- Tests: moto 13. Not yet played at chaos 3.
- **Open direction question:** Play Mode (your window layout as a playable level), a beat-'em-up variant, or a standalone game. Knox to pick one; lock it as a decision.

## Knox
- Watch more laps and call the tuning. Capsule picks, tags and store copy now describe dirtbikes.
