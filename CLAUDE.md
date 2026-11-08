# BEZEL: repo canon

BEZEL is a Steam desktop companion game for Windows 11.

It owns two kinds of ground:
- **Bedrock:** permanent left and bottom bars, docked as AppBars. The left bar is SHAFT and the bottom bar is TRENCH.
- **Frontier:** every patch of bare desktop, drawn on a desktop underlay that sits behind all windows.

Miners grapple onto the active window and build in the gaps between windows. Dragging a window crushes whatever it sweeps over.

Co-built by KNX (operator) and SPC (AI). Knox doesn't hand-code: explain results in plain terms, and ask him to run things only when a spike needs his eyes on the desktop.

## Canon (read before work)
- `docs/adr/0001-runtime-stack.md`: architecture plus decision log D1–D41. **Binding.** If code and the ADR disagree, stop and flag it.
- `docs/plan/fest-2027.md`: schedule, scope cut and gates. Anything outside its §2 waits.
- `docs/plan/w1-runbook.md`: current tasks.
- This repo's `docs/` is canon for code. The claude.ai BEZEL project mirrors it. When the ADR changes here, say so in the summary to Knox so the mirror gets updated.

## Build environment (THOR)
- Windows-native PowerShell in `C:\Users\KNX\BEZEL`. The Win32 crates need MSVC.
- Toolchain verified 2026-11-07: `stable-x86_64-pc-windows-msvc`, rustc/cargo 1.94.1, Visual Studio 2026 Community with C++ tools.
- `cargo` may not be on PATH in every shell. Fall back to `$env:USERPROFILE\.cargo\bin\cargo.exe`.
- WSL only for the pure crates, with `CARGO_TARGET_DIR=target-wsl` so it doesn't clobber MSVC artifacts.

## THOR is shared: leases
- The tool is `C:\Users\KNX\CAGED\tools\thor_lease.ps1`. **Its header is canon.** Read it before anything non-trivial.
- Run `status` before a full build or anything that puts load on the GPU. THOR runs parallel CAGED sessions.
- Full builds (the first `windows` crate build, release builds): `build` lease.
- Timed measurements (spikes S4 and S6, any fps or frame-cost numbers): `gpu` lease on a quiet GPU.
- Hold leases honestly: a real id, a real estimate, renew when running long, release when done.
- **Never `-Force` someone else's lease.** Ask Knox.

## Invariants (ADR §2.7). Breaking one is a bug.
1. No surface paints over another app's window, except BAND: at most 12 px at the current art scale, on the target's **left** client edge only, stopping 24 px short of the bottom, click-through, sliding away from the cursor. Never the right edge.
2. Window geometry changes only on events, never per frame. All motion happens inside surfaces.
3. When a fullscreen app is in the foreground on a monitor, that monitor's bars and BAND hide and its underlay stops presenting.
4. Topmost canopy surfaces are always fully click-through. No per-pixel click-through anywhere.
5. One coordinate space: physical pixels. The process is Per-Monitor-V2 DPI aware.
6. **Never write the system wallpaper.** No `SPI_SETDESKWALLPAPER`, no `IDesktopWallpaper::SetWallpaper`. Read only.

## Exit paths
Every exit path runs `ABM_REMOVE` on both AppBars and detaches every underlay window: normal quit, panic hook, Ctrl+C, console close, `WM_ENDSESSION`. A leaked work-area reservation or a stuck underlay is the worst bug this game can ship.

## Trust rules (no exceptions)
- **Input is counts only.** Raw Input with `RIDEV_INPUTSINK`. The keyboard path calls `GetRawInputData` with `RID_HEADER` **only**. Re-check this on every change to `os-win32/src/pulse.rs`.
- **Never** use `SetWindowsHookEx` or any low-level hook, DLL injection, or reads of another process's memory.
- Window tracking uses out-of-context WinEvent hooks only (`WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS`).
- Traces and logs record window **rects and ids only**. Never titles, class names or process names. Pulses are never written to disk or sent anywhere.

## Code layout (ADR §7)
- All `unsafe` lives in `os-win32` and `render-d2d`. Only those two crates may depend directly on `windows`.
- The `windows` crate is pinned to **one exact version** (`=x.y.z`), recorded in the workspace `Cargo.toml`. Upgrades are deliberate, never casual.
- Dependency rule: `core ← physics ← sim`, `core ← layout`, `core ← render-api ← {render-d2d, render-null}`, `core ← os-api ← {os-win32, os-fake}`, `app ← everything`. `tools/check-deps.ps1` fails if `windows` appears in the dependency tree of `bezel-sim`, `bezel-layout` or `bezel-physics`.
- Spikes go in `crates/os-win32/examples/spike_sN_<name>.rs`. Record each verdict (pass/fail, numbers, notes) in ADR §9.

## Working rules
- "Works" means it ran in this session. Otherwise tag it **[UNTESTED]**. Recalled API details that may have drifted get **[VERIFY]**.
- Interface signatures in ADR §8 are a design contract, not compiled code. Where reality forces a change, update the ADR in the same change.
- Stay in scope; raise adjacent improvements at the end. Flag scope creep on sight, including your own.
