# Week 1 runbook (Nov 9–15, 2026)

**Gate:** S9 verdict, either a working underlay or the rev-2 topmost fallback. Everything after week 1 depends on it.

## Already done (2026-11-07)
- Steamworks paperwork: done (Knox). No fee paid yet; that happens at the trailer-ready gate (D39).
- `C:\Users\KNX\BEZEL` created; git repo initialized; private GitHub remote created (Spec).
- Toolchain verified: `stable-x86_64-pc-windows-msvc`, rustc/cargo 1.94.1, Visual Studio 2026 Community with C++ tools.
- Lease tool present at `C:\Users\KNX\CAGED\tools\thor_lease.ps1`. At 19:31 THOR was busy with CAGED work (editor and gpu leases held, two builds queued), so check `status` before the first build.

## Kickoff prompt
Paste this into a Claude Code session opened in `C:\Users\KNX\BEZEL` (Windows PowerShell, not WSL):

> Read `CLAUDE.md`, `docs/adr/0001-runtime-stack.md` and `docs/plan/w1-runbook.md`. Execute week-1 tasks T1 through T5 in order. Follow the lease rules in CLAUDE.md before the first full build. When T3 builds, stop and tell Knox exactly how to run the S9 spike and what to look for, then wait for his report. Do the same for T4. Record both spike verdicts in ADR §9.

## Tasks (code session)

**T1. Scaffold** (ADR §7)
- Workspace `Cargo.toml` with every crate from §7 as a stub.
- `rust-toolchain.toml` pinned to 1.94.1.
- `.cargo/config.toml` with a static CRT for MSVC.
- `windows` crate pinned to an exact current version [VERIFY on crates.io at scaffold time], with the feature set trimmed to what S9 and S10 need.
- `tools/check-deps.ps1` enforcing the dependency rule.
- *Done when:* `cargo check --workspace` passes on THOR and `tools/check-deps.ps1` passes.

**T2. Pure crates**
- `core` types from §8.
- `layout::OcclusionGrid`: `new`, `apply`, `sweep`, `is_bare`, `platforms`, `rebuild`.
- *Done when:* `cargo test -p bezel-layout` passes, including a property test showing that incremental `apply` matches `rebuild` after random sequences of window changes, a sweep test, and a platform-detection test.

**T3. S9 spike** (`crates/os-win32/examples/spike_s9_underlay.rs`)

What it does:
- A console app. Console keys: `t` toggles transparent mode on the current monitor, `n` cycles monitors, `r` forces a reattach, `q` quits.
- On launch it logs: the desktop structure (Legacy or Raised), each monitor's id, rect and DPI, and whether a live wallpaper or widget already occupies that monitor.
- It attaches one underlay per monitor beneath the desktop icons. Each draws a per-monitor tinted checkerboard, a moving square, and a label.
- Opaque mode repaints the real wallpaper underneath first. On a monitor with a live wallpaper, it tries transparent mode.
- It tries a DirectComposition target first. If that fails, it falls back to a flip-model swap chain, and logs which presenter worked.
- On every exit path it detaches cleanly and never touches the wallpaper.
- It logs to `spike_s9.log`.

*Done when:* it builds and Knox's run produces a verdict (checklist below).

**T4. S10 spike** (`crates/os-win32/examples/spike_s10_occlusion.rs`)

What it does:
- Global out-of-context WinEvent hooks, filtered to top-level windows, feeding `layout::OcclusionGrid`.
- Once per second it logs: raw events, filtered events, windows tracked, and cells swept during a drag.
- Every 5 s it compares the grid against a full rescan and logs mismatched cells.
- A small debug window shows the grid, bare cells versus covered.
- It never logs titles, class names or process names. It logs to `spike_s10.log`.

*Done when:* Knox's 10-minute session is logged and the numbers are recorded in ADR §9.

**T5. Store groundwork**
- `docs/marketing/store-copy-v1.md`: short and long description; plain-language privacy copy (activity counts only, window positions only, wallpaper never changed, nothing leaves the PC); honest about what's in the build versus planned.
- `docs/marketing/capsule-prompts.md`: THOR pipeline prompts for capsule art, ready for the week-2 style frame.
- *Done when:* both files exist. Knox reviews them in week 2.

## Knox's run checklists

**S9 (about 10 minutes):**
- [ ] Every monitor shows the checkerboard where the desktop is bare, and windows cover it normally.
- [ ] Desktop icons are visible on top of it and still clickable.
- [ ] Restart Explorer (Task Manager → Windows Explorer → Restart). The underlay comes back within a few seconds.
- [ ] Change the wallpaper on one monitor. The underlay picks up the new one.
- [ ] Switch virtual desktops (Win+Ctrl+→ and back). It's still there and correct.
- [ ] On the Wallpaper Engine/Lively monitor, press `t`. The live wallpaper shows through with the checkerboard on top, without flicker.
- [ ] Press `q`. Every wallpaper is exactly as it was and nothing is left behind.
- [ ] Send Spec `spike_s9.log` plus screenshots.

**S10 (10 minutes of normal use):**
- [ ] Work normally: open and close apps, maximize, snap, minimize, switch virtual desktops.
- [ ] Fling a window fast across the screen a few times.
- [ ] Bring up any overlay apps you use (Discord, the NVIDIA app, PowerToys) and confirm they don't paint the whole grid as covered.
- [ ] Send Spec `spike_s10.log`.
