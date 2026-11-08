# Phase 2 runbook (was "W2"; D44)

**Gate:** both bars and the underlay running on THOR's real desktop.
**Started:** 2026-11-07, straight after phase 1 closed.

## Already done in phase 1
- The occlusion grid and its tests (fest plan listed them under W2).
- S9 (underlay) and S10 (occlusion) verdicts in ADR §9.

## Rulings that opened this phase
- **D42:** `Platform` moved to `core`; `sim` may depend on `layout`. Confirmed by Knox.
- **D43:** art uses a custom BEZEL style built on imagegen's `illustration` preset (`assets/art/bezel-style.txt`).
- **D44:** weeks are phases.

## Tasks (code session)

**P2.1 Physics** (`bezel-physics`, pure)
- `RopeWorld`: verlet chains, fixed dt, `spawn` / `pin_end` / `cut` / `step` / `nodes` (ADR §8).
- `fall_step`: gravity, a parachute's terminal velocity, landing on the first platform below or on bedrock.
- *Done when:* `cargo test -p bezel-physics` passes, including rope length stability, a cut rope falling free and a fall landing on the right platform.

**P2.2 S3 AppBar spike** (`crates/os-win32/examples/spike_s3_appbars.rs`)
- TRENCH (bottom, 112 px) and SHAFT (left, 144 px) as AppBars on the home monitor next to the Windows 11 taskbar. TRENCH registers first; SHAFT stops at TRENCH's top.
- Handles `ABN_POSCHANGED`, `ABN_FULLSCREENAPP`, re-registers on `TaskbarCreated`, runs `ABM_REMOVE` on every exit path.
- Logs the home monitor's work area before, during and after.
- A harness script (`tools/s3-harness.ps1`) runs it with no hands: launch → check the work area → restart Explorer → check → force-kill → check for a leaked reservation → clean up → confirm the work area matches the start.
- *Done when:* it builds, and the harness has run on THOR. **The harness restarts Explorer and resizes maximized windows, so Spec needs Knox's OK before each run.**
- **Status: PASS 2026-11-07 (5/5, no force-kill leak). ADR §9.**

**P2.3 S0 corner spike** (`crates/os-win32/examples/spike_s0_corner.rs`)
- Two bar windows on one DirectComposition device, one `Commit()` per frame, a sprite crossing the TRENCH↔SHAFT corner at 60 fps.
- Logs commit timing; a tear can only be seen, so Knox looks once.
- *Done when:* it builds and the verdict is recorded.
- **Status: PASS 2026-11-07 (Knox: no tear; one device, one Commit, max Commit 2.6 ms). ADR §9.**

**P2.4 Style frame** (art, D43)
- `tools/bezel-imagegen.ps1`: appends the BEZEL style suffix and runs imagegen detached. The imagegen daemon takes the gpu lease itself; wrapping it in a lease deadlocks (learned 2026-11-07).
- **Style frame LOCKED: candidate 4** -> `assets/art/style-frame.png` (Knox, 2026-11-07).
- **Status:** header batch done (8 candidates, seed 1589715672, `X:\imagegen\out6-11-07_cozy_dark_*`). The close-up batch was killed to free the gpu queue; re-run it on a quiet THOR. Next prompt: the window frames are the cliffs, nothing inside the windows, generic title bars.
- Batch: the header-capsule prompt, plus a miner/rig close-up, 8 images each.
- *Done when:* the images are on disk and listed for Knox to pick from.

**P2.5 Underlay presenter** (`os-win32::DesktopLayer` + `render-d2d` underlay path)
- Promote the S9 path: Raised/Legacy detection, the child window under the icons, the DComp presenter with the flip fallback, transparent mode over a foreign occupant, reattach on `TaskbarCreated`, and detach on every exit path.
- Where §8's `DesktopLayer` doesn't fit reality, the ADR changes in the same commit.
- *Done when:* it builds, the pure parts have tests, and a minimal demo binary shows it on THOR.

**P2.6 Bars docked** (after P2.2 and P2.3 pass)
- TRENCH and SHAFT as real surfaces through `ShellDock` and the bar presenter.
- **Built 2026-11-07:** `os-win32::dock::Win32ShellDock` (from S3), `render-d2d` bar presenter (from S0: DComp surfaces, one Commit for every bar), `crates/app/examples/bedrock_demo.rs` (bars + underlays together = the phase gate). Gate run done (below).

## Gate result (2026-11-07 22:44, gpu job `bezel-p2.j1-1107-223651`)
**PASS. Phase 2 gate met.** Knox watched the run: "perfect". `bedrock_demo --seconds 20` on THOR's real desktop: TRENCH docked at (0,1288)-(2560,1400) and SHAFT at (0,0)-(144,1288), the home work area went to (144,0)-(2560,1288), and the underlay attached on all three monitors (transparent over the live wallpaper). Every renderer attach and all 788 frame submits succeeded (stderr empty), 0 re-docks, 0 underlay resets, and the work area was back to (0,0,2560,1400) on exit. Knox confirmed the bars and underlay drew correctly.

## Knox
- ~~Confirm D42.~~ Done.
- OK each S3 harness run (it restarts Explorer).
- ~~One look at S0 for tearing.~~ No tear.
- ~~Pick the style frame.~~ Candidate 4.
- Review the store copy and capsule prompts (questions at the bottom of the store copy).
- Mirror the ADR to the claude.ai project.
