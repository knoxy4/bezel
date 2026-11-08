# Phase 3 runbook (was "W3"; D44)

**Gate:** one full loop on THOR's real desktop, ugly is fine. Miners hook the active window, a rig and ropes go up,
units land on windows as platforms, a drag crushes the rig and 60% comes home as salvage, and typing heats the foundry,
which smelts scrap into fuel.
**Started:** 2026-11-07, straight after the phase-2 gate.

## Carried over
- The bars must pace to the D10 tiers (24 / 30 / 60 fps), not the display refresh (S0 finding).
- Re-run the style close-up batch on a quiet THOR; next art prompts use the frames-are-terrain rule.

## Tasks

**P3.1 Pulses** (`os-api::InputPulseSource`, `os-win32::pulse`)
- Raw Input with `RIDEV_INPUTSINK` on a message-only window, on its own thread. 100 ms `PulseBatch` buckets over a channel.
- **Trust rule:** the keyboard path calls `GetRawInputData` with `RID_HEADER` only. The header's type decides; only mouse
  input is read with `RID_INPUT`. A key press and its release are two keyboard events; the sim weights counts, it never
  sees keys.
- Pause flag (tray toggle later; D5).

**P3.2 Window tracker** (`os-api::WindowTracker`, `os-win32::winevent`), promoted from S10
- Global out-of-context WinEvent hooks on a shell-watch thread; filtered to top-level windows; DWM extended frame bounds;
  the S10 coverage filter (visible, not cloaked, not minimized, not ours, not the shell desktop, not click-through, not a
  fully transparent layered window).
- Emits `OsEvent::WindowRect { id, rect, cause }` and `OsEvent::Foreground`. `snapshot` for boot and rescans.

**P3.3 Sim** (`bezel-sim`, pure)
- Ledger (scrap, fuel, salvage), the foundry (D6 weights heat it; heat smelts scrap into fuel).
- Canopy frontier mode: a rig on the target window's top edge when ≥ 88 px of bare cells sit above it; ropes down its
  sides through bare cells; miners mine at the rig, climb down, fall under parachutes, land on platforms (D24) or bedrock,
  and bring scrap home.
- D21: a drag sweep crushes; anything else that covers buries; uncovering unburies. Anything anchored to a window that
  moves or vanishes snaps. D25: a crush returns 60% of build cost as salvage that falls home.
- Fixed 20 Hz, seeded, no wall clock (D13). *Done when:* `cargo test -p bezel-sim` covers heat, crush, bury/unbury,
  salvage and determinism.

**P3.4 App loop** (`crates/app`)
- Wires the dock, desktop layer, renderer, tracker, pulses, occlusion grid and sim. Pacer tiers from D10.
- Every exit path undocks and detaches.

**P3.5 Gate run** on THOR (gpu lease), Knox watches and plays: switch windows, drag one through a rig, type.

## Status (2026-11-07)
- P3.1 pulses: `os-win32::pulse::Win32Pulses`. Trust rule checked: the only `RID_INPUT` read sits behind a mouse-type header check.
- P3.2 tracker: `os-win32::winevent::Win32WindowTracker` (shell-watch thread, S10 filter).
- P3.3 sim: `bezel-sim` foundry, rigs, ropes, miners, platforms, crush/bury/snap, salvage; 8 tests pass (heat, rig needs open sky, miners deliver, platform landing, drag crush with 60% salvage home, bury/unbury, anchor snap, determinism).
- P3.4 app: `bezel-app` wires everything; pacer on the D10 tiers (fixes the S0 carry-over).
- P3.5 run 1 (23:14): foundry worked; **no rig ever deployed**. `bezel-app --probe` (read-only) explained it: of 8 counted windows, 3 were taskbars (now excluded as targets), the real windows sat within 88 px of the top of their monitor (`EdgeTooHigh`) or were maximized, and monitor 2 was 0% bare. Only one real window on the desktop could hold a rig. The app now logs to `bezel.log`, including why each target got no rig.
- P3.5 run 2: Notepad's title bar at y=32, no rig (correct). Fixed: the 88 px sky check stays on the window's own monitor.
- P3.5 runs 3–4 (23:20–23:30, from `bezel.log`): **the loop ran.** Rigs deployed on Notepad once it sat with open desktop above it (mid-screen it was `SkyCovered`: the CAGED window behind it covered the sky). Run 3: 2 rigs, 2 crushes (moving the anchor snaps it), all 36 salvage home, 6 scrap hauled by miners, fuel 42. Run 4: deploy, crush, rebuild, then **buried** when something covered all three monitors and **dug out** when it cleared.
- Fixed after runs 3–4: (1) economy dead end: run 3 ended at scrap 0 with no rig and no income. The foundry now keeps one rig's cost in reserve, and with no rig standing the next one takes whatever scrap is left. (2) Rigs built mid-drag snapped on the next nudge; a target must now sit still 1 s first. Sim tests: 11 pass.
- Run 4 kept running after the gpu lease went to a CAGED `-game` capture: the app must exit when its lease is gone (open item).
- **P3.5 run 5 (23:4x, primary 84% bare): GATE MET.** Knox saw the rig (crane), ropes down both sides of Notepad, miners parachuting home; log: deploy, bury, dig out, crush, salvage home. Typing heated the foundry in every run.
- Knox's notes from run 5: ropes "need little cranes holding them" → added davits (post on the window's top corner, jib out over the side, rope hangs from the tip; sim + extract, tested). Miners "disappear" at the bottom: that's delivery + resting in the trench row; make it readable in phase 4 (hatches).
- **Design finding (needs Knox):** on a real work layout, frontier mode almost never has a target. Windows hug the top edge or are maximized, and a maximized window behind everything covers its whole monitor in the grid (coverage ignores z-order, by design). Wall-run (maximized targets, fest cut, phase 4) and band (stretch) are the modes that cover this; the trailer can stage a layout, but players won't.
