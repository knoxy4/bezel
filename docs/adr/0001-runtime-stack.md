# ADR-0001: BEZEL runtime stack & core architecture

**Status:** Accepted, revision 4. Stack, territory model and surface model are locked, and every design fork is ruled. Spikes S0–S10 gate the fest cut. Target: Steam Desktop Companion Fest, January 14–18, 2027 (see `docs/plan/fest-2027.md`).
**Date:** 2026-11-07
**Deciders:** KNX / SPC

---

## TL;DR

**Stack: native Rust, no engine.** `windows-rs` + DirectComposition + Direct2D, laid out as a Cargo workspace with ports and adapters. This hasn't changed since revision 1.

**Revision 3 headline: BEZEL owns two kinds of ground.**
- **Bedrock** is the permanent left and bottom bars on the home monitor: SHAFT is an AppBar on the left, TRENCH is an AppBar on the bottom. Maximized windows fit inside them and can never cover them. The foundry and the shaft live here.
- **Frontier** is every patch of bare desktop on every monitor. It's drawn on a **desktop underlay**: an opaque layer behind every window and behind the desktop icons, on top of the player's own wallpaper. Windows hide it for free. Miners build outposts there, string ropes and drop salvage. **Dragging a window across their work crushes it**; anything else that covers it only **buries** it until it's uncovered again.

What comes for free from this:
- The catastrophe mechanic falls out of an occlusion grid.
- Windows become platforms that units can land on.
- Most of the always-on-top canopy windows from revision 2 go away, and with them most of the click-through risk.

The cost is the underlay itself. It relies on an undocumented desktop window structure that Windows 11 24H2 changed (spike S9).

**Revision 4 headline: the fest cut.** BEZEL is going to Steam's Desktop Companion Fest (January 14–18, 2027) with a Coming Soon page and a trailer. A demo goes too only if it passes the January 4 gate. All remaining design forks are now ruled:
- The drill bores *up* and loot falls home.
- Premium price, one-time purchase.
- Pure endless play, no offline progress.
- The player gives light directives from the bars.
- Shaft monsters can breach onto the frontier (post-fest).
- The THOR pipeline makes all the art.
- On monitors running a live wallpaper, the frontier is drawn transparently over it, if S9 shows that works.

Tags: **[VERIFY]** means remembered and may have drifted. **[UNTESTED]** means never run. Nothing in this document has been run yet.

---

## 0. Decision log

| # | Fork | Ruling | Provenance |
|---|---|---|---|
| D1 | Canopy on maximized/snapped windows | **Edge-hug policy.** Rev 3 modes: frontier, wall-run, band (§2.3). | Knox |
| D2 | Dock mode | **Reserve both bars** (AppBars). Yield remains a tray option. | Knox |
| D3 | Shaft side | **Left.** | Knox, after the Gemini mockup |
| D4 | Monitors | *Amended by D20:* the tray picks the **home monitor** (where the bedrock sits); the frontier runs on every monitor; egui has a per-monitor frontier toggle. | Knox; toggle is Spec's call |
| D5 | Input consent | **On by default, disclosed.** First-run card with one-click off, plus a tray toggle. | Knox |
| D6 | Input weights | Keyboard ×3, wheel ×3, clicks ×2, mouse travel ×2 | Knox |
| D7 | Fullscreen stand-down | Sim keeps running; input counting pauses. | Spec's pick, kept |
| D8 | Art | **Hybrid:** pixel sprites, smooth ropes and FX | Knox |
| D9 | Tone | Spectacle 100 / presence 99 | Knox |
| D10 | Idle footprint | **Always on, no exceptions.** 24 fps floor whenever anything is visible. | Knox (ruled; don't re-raise) |
| D11 | OS target | **Windows 11 only** | Knox |
| D12 | M1 build order | Canopy grapple → trench foundry → shaft → terrarium | Knox |
| D13 | Determinism | Per build | Spec's pick, kept |
| D14 | Steamworks | In M1 | Knox |
| D15 | Settings UI | egui panel, opened on demand | Knox |
| D16 | Fest headline | Canopy scavengers | Spec's call |
| D17 | Bar sizes | Trench 112 px, shaft 144 px at 100% scaling | Spec's call |
| D18 | Drop columns, 88 px ledge, ledge rigs | *Superseded by D19–D24:* rigs and falls move to the underlay. The rev-2 topmost windows (LEDGE, LANES, DROP) survive only as the fallback if S9 fails. | Spec |
| **D19** | **Hard constraint amended** | **Only transparent topmost overlays are banned. An opaque desktop underlay at the bottom of the stack is allowed.** The ban's reasoning (DWM blend cost, VRR) is kept. | Knox (Spec's pick, kept) |
| **D20** | Territory | **Bedrock** = bars on the home monitor. **Frontier** = bare desktop on every monitor. | Knox (Spec's pick, kept) |
| **D21** | Crush rule | **Drags crush, everything else buries.** A crush is a user move or resize (the move/size loop), including everything the window sweeps over. Opening, restoring, maximizing, keyboard snaps and virtual-desktop switches bury. | Knox (Spec's pick, kept) |
| **D22** | Underlay backdrop | **The player's own wallpaper**, read-only, with the game drawn on top. | Knox (Spec's pick, kept) |
| **D23** | Frontier input | **Watch-only at M1.** All interaction happens in the bars. | Knox (Spec's pick, kept) |
| **D24** | Windows as terrain | **Yes.** A window's top edge is a platform wherever the cells above it are bare. | Knox (Spec's pick, kept) |
| **D25** | Crush salvage | **60%** of build cost comes back as salvage that miners carry home. | Knox |
| DF-2 | Which way is "down" in the shaft | **Flip the fiction:** the trench is the bottom of the world, the drill bores up, and loot falls home (§2.6). | Knox (Spec's pick, kept) |
| **D26** | Fest target | **Steam Desktop Companion Fest, January 14–18, 2027** | Knox (note) |
| **D27** | M1 purpose | A vertical slice that can be shown publicly. D37 scopes it into the fest cut. | Knox |
| **D28** | Art source | **THOR pipeline for everything:** sprite-forge renders plus FLUX paintover. The store page needs Steam's AI-content disclosure [VERIFY current rules]. Generation runs take a GPU lease. | Knox |
| **D29** | Business model | Premium, one-time price | Knox (Spec's pick, kept) |
| **D30** | Player control | Light directives from the bars: priorities, build queue, squad loadout. Units act on their own. | Knox (Spec's pick, kept) |
| **D31** | Frontier threats | **Shaft monsters sometimes breach onto the frontier.** Defenses fight them automatically. Post-fest. | Knox |
| **D32** | Frontier structures | Mining rigs on window edges, rope lines and zip lines between windows, scrap caches, refineries, defenses/turrets | Knox |
| **D33** | Endgame | **Pure endless, no resets** | Knox |
| **D34** | Offline progress | **None.** The sim runs only while BEZEL runs; `catch_up` is removed. | Knox |
| **D35** | Audio | Silent by default; sounds are opt-in | Knox (Spec's pick, kept) |
| **D36** | Transparency | A live "what BEZEL can see" panel in settings: pulse counts, window rectangles (no titles), wallpaper path | Knox (Spec's pick, kept) |
| **D37** | Fest cut | **Coming Soon page plus trailer. Demo only if it passes the January 4 gate.** Scope is in `docs/plan/fest-2027.md`. | Knox (Spec's pick, kept) |
| **D38** | Live-wallpaper monitors | **Frontier drawn transparently over live wallpapers** (Wallpaper Engine, Lively). If S9 can't do it, those monitors are excluded by default. Knox's top monitor is one of them. | Knox (Spec's pick, kept) |
| **D39** | Steam Direct fee timing | **Paid at the trailer-ready gate** (target early December). No money changes hands before S9 passes and the hero loop is on screen. | Knox (Spec's pick, kept) |
| **D40** | Store presence | **Coming Soon page only, release date "To be announced."** No release until Knox calls the game shippable. | Knox |
| **D41** | Development app id | **Valve's generic test app id** until BEZEL's own app exists [VERIFY id and limits]. Builds under it are dev-only and are never distributed. | Knox |
| **D42** | Dependency rule vs §8 signatures | **`Platform` moves to `core`; `sim` may depend on `layout`.** `physics::fall_step` takes platforms and `sim` consumes `Zones`/`CellDelta`; both are pure, so the Win32 firewall is unchanged. | Knox (Spec's pick, kept) |
| **D43** | Art style | **Custom BEZEL style built on the imagegen `illustration` preset** (suffix kept in `assets/art/bezel-style.txt`, run as `--style raw` with the preset's steps/guidance). The shared imagegen tool is not modified. **Style frame locked 2026-11-07: candidate 4 of the header batch, seed 1589715672, `assets/art/style-frame.png`.** Every later art run passes it as `--ref`. | Knox |
| **D44** | Schedule | **Weeks become phases.** Work runs ahead of the calendar; the fest dates and the money gates stay fixed. | Knox |

**Spec's calls in rev 3, all reversible:**
- Sweep-crush is sampled from the drag's own location events at ≤ 30 Hz.
- Coverage is tracked as per-cell window counts, not by rescanning.
- Other apps' click-through/transparent overlay windows are ignored when computing coverage.
- ~~If a live-wallpaper app is detected, M1 warns instead of trying to coexist.~~ Superseded by D38.
- Frontier units enter the bedrock through trench hatches, which hide the seam between the two composition paths.
- BEZEL never writes the system wallpaper (invariant 6).

**Spec's calls in rev 4, all reversible:**
- Steamworks SDK wiring (D14) is off the fest's critical path. The store page only needs the Steamworks *app* to exist. The SDK gets wired in only if the demo ships; otherwise it happens right after the fest.
- Breaches spill out of the shaft's outer wall onto the home monitor's frontier first.
- The fest-cut shaft is "shaft lite": the drill bores up on fuel and drops a core on a timer, with no battle. The terrarium is ambient set dressing until after the fest.
- The edge-hug band (which needs S1) is a stretch goal for the fest cut.

**Concept references:**
- Gemini mockup (2026-11-07): outpost on the window's top edge, ropes down the frame, elevator handoff at the corner, hybrid art.
- Knox's 3-monitor screenshot (2026-11-07): frontier territory marked in red. It matches what the underlay exposes.

---

## 1. Context

### 1.1 Hard constraints (from the brief, amended)
- **No transparent topmost overlay covering a whole monitor.** It costs DWM blending and breaks VRR. An opaque underlay at the bottom of the stack is allowed (D19).
- Input is pulse counts only. Never which key.
- Strict module isolation: window tracker, input pulses, physics/grapple, auto-battler sim and renderer can each be swapped out.
- Lives on the edges of the workspace, never steals focus, never covers scrollbars. D2 + D3 satisfy this for the bars. The underlay sits under everything, so it can't cover anything.

### 1.2 The canopy and frontier are mid-screen
Units on the active window's frame and structures in desktop gaps both mean drawing in the middle of the screen. Rev 3 answers with a layered model: bedrock bars at the edges, the frontier underlay beneath everything, and exactly one topmost exception (BAND).

### 1.3 Any overlap breaks VRR
A topmost window over a fullscreen game knocks the game out of independent flip [VERIFY S6]. The underlay sits *beneath* games, so it can't do that. The bars and BAND hide whenever a fullscreen app is in the foreground.

---

## 2. Decision: territory and surfaces

### 2.1 Surfaces

| Surface | Layer | Geometry | Input | Lifetime |
|---|---|---|---|---|
| `TRENCH` | topmost AppBar | home monitor width × 112, bottom, above the taskbar | interactive | permanent |
| `SHAFT` | topmost AppBar | 144 × (work-area top → trench top), left | interactive | permanent |
| `UNDERLAY(m)` | **desktop layer, under the icons** | the full rect of monitor *m*; one per monitor with the frontier enabled | none at M1 (D23) | permanent while BEZEL runs |
| `BAND` | topmost | 12 × (title bar → bottom − 24), on the target's left client edge | click-through | band mode only |
| *fallback:* `LEDGE`, `LANE_L/R`, `DROP_0..3` | topmost | as in rev 2 | click-through | **only if S9 fails** |

Sizes are px at 100% scaling, multiplied by art scale = max(1, round(dpi / 96)). They live in `assets/tuning/surfaces.ron`.

**UNDERLAY in detail:**
- Opaque. Every frame, wherever it redraws, it paints the monitor's current wallpaper (read through the shell's wallpaper API: path, fit mode, background color) and then frontier content on top.
- It sits beneath every window and beneath the desktop icons, so the icons stay visible and usable.
- Only exposed dirty cells are redrawn. Covered cells are skipped, because the occlusion grid knows they're hidden.
- When BEZEL exits, the underlay windows are destroyed and the real wallpaper, which was never changed, shows through.

### 2.2 The occlusion grid
- **Per monitor:** a grid of 16 art-px cells. Each cell holds a **coverage count**: how many visible top-level windows overlap it. A cell is **bare** when its count is 0.
- **Incremental:** when a window's rect changes, subtract 1 from its old cells and add 1 to its new cells. The work is proportional to the changed area; there's no full rescan.
- **Rect source:** `DWMWA_EXTENDED_FRAME_BOUNDS`, which excludes the invisible resize borders. Without that, a window's invisible borders would bury the cells next to it.
- **Which windows count:** visible, top-level, not cloaked, not minimized, not ours. **Excluded:** windows that are click-through (`WS_EX_TRANSPARENT`) or fully transparent layered windows, so other apps' overlays (Discord, the NVIDIA app, PowerToys and similar) don't bury the whole desktop [VERIFY S10].
- **Output:** a `CellDelta` per change (newly covered cells, newly bare cells) tagged with its **cause**. The sim applies the D21 rule to the cause.
- **Platforms (D24):** a window's top edge, wherever the cells directly above it are bare.

### 2.3 Canopy modes (D1, rev 3)

| Target state | Mode | Where it draws | Paints over any app window? |
|---|---|---|---|
| The target has bare cells around its frame | **Frontier** | UNDERLAY: an outpost on its top edge (needs ≥ 88 px of bare cells above), ropes down its sides through bare cells, falls through bare cells | **No.** It's underneath by construction. |
| Maximized or snapped left (flush against SHAFT) | **Wall-run** | Inside SHAFT: units climb the inner wall and hook the target's top-left corner | No |
| No bare cells around it and not flush against the shaft | **Band** | BAND on its left client edge, plus a hook sprite | **BAND only:** click-through, low alpha, slides away when the cursor comes near |
| Ineligible | Dormant | — | — |

A rope or outpost that ends up behind *another* window is simply hidden behind it, which reads as depth. A rig whose cells get covered is buried or crushed by the D21 rule.

### 2.4 Crush and bury (D21, D25)
| Cause (from OS events) | Effect on newly covered cells |
|---|---|
| A user move or resize, from `MOVESIZESTART` to `MOVESIZEEND`, including **every cell the window sweeps over**, sampled from the drag's location events at ≤ 30 Hz | **Crush.** Structures are destroyed, ropes snap, units fall. 60% of build cost drops as salvage. |
| Open / show, restore, maximize, keyboard snap or other software move (`LOCATIONCHANGE` outside a move/size loop), virtual-desktop switch (cloak/uncloak) | **Bury.** Structures pause and go invisible; they dig out when the cells are bare again. |
| Close / hide / minimize / move away (cells become bare) | **Unbury.** Buried structures resume. |

Anything anchored *to* a window that gets grabbed (ropes, its top-edge outpost, units standing on it as a platform) snaps or falls at `MOVESIZESTART`, as in rev 2.

### 2.5 Bedrock seams
- **TRENCH ↔ SHAFT** (bottom-left corner): TRENCH registers first and gets the full width; SHAFT registers second, on the left, and stops at the top of the trench. The corner belongs to TRENCH. Both bars are drawn on one DirectComposition device, so a single `Commit()` updates both and the seam can't tear [VERIFY S0].
- **Frontier → bedrock:** the underlay and the bars render through different presenters, so a seam between them can be a frame apart. Frontier units enter through **hatches** along the top edge of the trench and in the shaft's outer wall, and the hatch animation hides the handoff.

### 2.6 Shaft fiction (DF-2, resolved)
The trench is the bottom of the world. The drill bores *up* through monster strata, and everything valuable falls home: scrap down the ropes and off window platforms, boss cores down the shaft. One gravity rule covers the whole game. Under D33 there's no finale: the bore climbs forever, and the strata keep getting harder.

### 2.7 Invariants (enforced in `layout`, tested there)
1. **No surface paints over another app's window, except BAND.** BAND is ≤ 12 px at the current art scale, sits on the target's *left* client edge only, stops 24 px short of the bottom, is click-through, and slides away from the cursor. The right edge is never touched. The underlay can't violate this because it's underneath everything.
2. **Window geometry changes only on events, never per frame.** Underlays are fixed per monitor. BAND and the bars move only on events. All motion happens inside surfaces.
3. **When a fullscreen app is in the foreground on a monitor,** that monitor's bars and BAND hide, and its underlay stops presenting (it's fully covered anyway).
4. **Topmost canopy surfaces (BAND, plus the fallback set) are always fully click-through.**
5. **One coordinate space: physical pixels.** The process is Per-Monitor-V2 DPI aware.
6. **BEZEL never writes the system wallpaper.** It only reads it. Writing it would change the player's settings and, under the 24H2+ desktop layout, destroy the window the underlay lives in. On every exit path, underlay windows are detached, alongside `ABM_REMOVE`.

### 2.8 Frontier economy and threats (D31, D32)
- **Structures:**
  - **Mining rigs** sit on window top edges and mine the target window.
  - **Rope lines** run down a window's side.
  - **Zip lines** span bare cells between two windows' platforms.
  - **Scrap caches** store salvage on the frontier, and can be crushed.
  - **Refineries** process scrap before the haul home.
  - **Defenses and turrets** fire automatically at breaching monsters.
- **Breaches (post-fest):** now and then the shaft breaks through its outer wall. Monsters spill onto bare cells of the home monitor's frontier and walk across window platforms. Turrets and miners fight them on their own; under D30 there are no direct orders. A window drag crushes monsters too, so the player's catastrophe doubles as a weapon.
- Every frontier structure follows the crush and bury rules in §2.4.

---

## 3. Options considered

### Option A: Tauri v2 (Rust + WebView2 multi-window, PixiJS/Canvas)
| Dimension | Assessment |
|---|---|
| Idle footprint | **Bad.** Chromium runtime: browser + GPU + renderer processes [VERIFY figures]. |
| Many small surfaces | **Bad.** Each surface would be its own WebView2. |
| Sync across surfaces | **Bad.** Separate renderers per webview [Probably], with skew over IPC. |
| External window rects / DPI | Two coordinate systems: logical (webview) vs physical (Win32). |
| Iteration | **Best.** Vite hot reload. |
| Input | `rdev` uses a global low-level keyboard hook [VERIFY], which is the keylogger shape this project must avoid. |

### Option B: Godot 4 (C#/GDScript + GDExtension)
| Dimension | Assessment |
|---|---|
| Idle footprint | OK in low-processor mode, but every extra Window costs a swapchain and viewport. |
| Many small surfaces | Weak. Each `Window` is a full swapchain plus viewport. |
| Sync across surfaces | OK. Same process, but windows present separately. |
| Win32 control | Weak. AppBar docking, WinEvent hooks, Raw Input and the desktop underlay all need a native bridge. Built-in transparency and passthrough on Windows have field reports of flakiness. |
| Iteration | Weak for this workflow: it's editor-centric. |
| Art tooling | **Best.** |

Runner-up. Becomes the fallback if the native spikes fail.

### Option C: Pure Rust engine (Bevy / Macroquad / raw wgpu)
Bevy brings a full ECS and render graph to draw a few strips. It compiles slowly and its API churns [VERIFY]. Macroquad is single-window. Raw wgpu stays possible later as a backend behind `Renderer`.

### Option D: Native Rust, `windows-rs` + DirectComposition + Direct2D (**winner**)
| Dimension | Assessment |
|---|---|
| Footprint | **Best available.** Dirty-rect redraws; one commit for the bars; partial presents for the underlays. The floor is 24 fps under D10 [UNTESTED, S4]. |
| Many small surfaces | **Best.** Shared D3D11 and DirectComposition devices. |
| Sync across surfaces | **Best** for the bars (one atomic `Commit()`). The frontier↔bedrock seam is hidden by hatches. |
| External window rects / DPI | **Best.** Physical px throughout. |
| Win32 control | **Best.** It *is* Win32, including the desktop layer. |
| Iteration | Good. Pure crates test anywhere; hot-reloaded tuning and atlases; diorama mode. |
| Art tooling | Weak. Bring your own pipeline. |

---

## 4. Trade-off analysis

D wins outright on footprint, sync, rects/DPI and Win32 control, and the desktop underlay makes that last point decisive: no engine exposes it. D's losses are art tooling and UI hot reload, and both can be fixed with tooling.

The costs:
- **Unsafe Win32 code that Knox can't review by eye.** Contained by two crates, a pinned `windows` version, spikes and fakes.
- **New in rev 3: dependence on an undocumented desktop layer.** Contained by supporting both known structures, recovering after Explorer restarts, never writing the wallpaper, and the rev-2 topmost canopy as a fallback.

---

## 5. Win32 edge cases

### 5.1 Clicking without stealing focus
- Bars and BAND are `WS_POPUP` + `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP`. `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`. Never call `SetForegroundWindow`/`SetFocus`; every `SetWindowPos` includes `SWP_NOACTIVATE`.
- Underlays take no input at M1. Clicks on bare desktop go to the icons layer, as normal.
- Hover uses `TrackMouseEvent`. Drags use `SetCapture` [VERIFY S2]; fallback is to keep drags inside the surface.
- Tray menu: Pause counting · Dock mode · Home monitor ▸ · Settings… · Quit.
- Settings opens the egui panel: a normal, focusable window, created on open and destroyed on close. That panel and the first-run card are the only things that take focus, and only when the user asks.

### 5.2 Docking: permanent left and bottom bars (D2 + D3)
- TRENCH (bottom) and SHAFT (left) register as AppBars on the home monitor through `SHAppBarMessage`. Maximized and snapped windows lay out inside the remaining work area, so scrollbars are never covered.
- Handle: `ABN_POSCHANGED`, `ABN_FULLSCREENAPP`, `ABN_STATECHANGE`; re-register on `"TaskbarCreated"`; `ABM_REMOVE` on every exit path [VERIFY S3: cleanup when the process dies uncleanly]; Windows 11 stacking with the taskbar [VERIFY S3]; re-dock on display, DPI, work-area or home-monitor changes.
- Fallback if S3 fails: Yield mode, with the shaft still on the left.

### 5.3 The desktop underlay (D19, D22)
- **Two known desktop structures:**
  - **Legacy:** send the undocumented `0x052C` message to `Progman`, which creates a `WorkerW` behind `SHELLDLL_DefView` (the icons). Reparent into that `WorkerW`.
  - **Raised (Windows 11 24H2+ servicing):** `Progman` uses `WS_EX_NOREDIRECTIONBITMAP`, the icons become a layered child of `Progman`, and the wallpaper is drawn by a child `WorkerW`. Create our window as `WS_CHILD` (never combined with `WS_POPUP`), reparent into `Progman` itself, and place it directly below `SHELLDLL_DefView`.
- **Detect** the structure at startup and after every Explorer restart. Reattach on `"TaskbarCreated"`.
- **Never write the wallpaper** (invariant 6): `SPI_SETDESKWALLPAPER` destroys the `WorkerW` under the raised layout. Watch for wallpaper changes (`WM_SETTINGCHANGE`, virtual-desktop switch) and **re-read** the wallpaper, then reattach if the window was torn down.
- **One child window per monitor** with the frontier enabled, so each can render at its monitor's own DPI.
- **Presenter:** whether DirectComposition targets work on a child of the desktop window is unknown [VERIFY S9]. Fallback is a flip-model swap chain with dirty-rect presents. The underlay is opaque either way, so it never needs alpha blending.
- **Coexistence (D38):** if a live-wallpaper app (Wallpaper Engine, Lively) or a desktop-pinned widget already sits in that window tree, the underlay on that monitor switches to **transparent mode**. It uses per-pixel alpha and doesn't repaint the wallpaper, so the other app's wallpaper shows through with the frontier drawn on top. If S9 shows transparent mode can't hold up (flicker, or losing its place in the window stack on reattach), those monitors are excluded by default. The player can still opt in to opaque mode.
- **Fallback if S9 fails entirely:** the rev-2 topmost canopy set (LEDGE, LANES, DROP). The frontier shrinks to what those windows can reach.

### 5.4 Window tracking: all windows, event-driven
- `SetWinEventHook(..., WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)` runs on the shell-watch thread. No injection.
- It's now **global**, not scoped to the target's process, because the occlusion grid needs every top-level window.
- Events: show/hide, create/destroy, `LOCATIONCHANGE`, `MOVESIZESTART`/`MOVESIZEEND`, minimize start/end, cloak/uncloak, foreground.
- **Filter hard.** Keep only `OBJID_WINDOW` + `CHILDID_SELF` events on visible top-level windows. Caret and cursor location changes dominate the raw stream and must be dropped [VERIFY S10: event volume].
- **Events are hints; DWM is the truth.** For each dirty window, read its extended frame bounds once per frame at most.
- **During a move/size loop:** sample the dragged window's rect from its own location events, at ≤ 30 Hz, to build the sweep for crushes.
- **Target selection** (foreground window): flush against SHAFT → wall-run; bare cells around its frame → frontier; neither → band; ineligible → dormant.
- Elevated windows may not send events to a non-elevated hook [VERIFY S5]. If they don't, they count toward occlusion through a rescan on foreground change, and the canopy goes dormant on them.

### 5.5 Fullscreen stand-down (D7)
`FullscreenGuard` combines three signals: `ABN_FULLSCREENAPP`, `SHQueryUserNotificationState`, and "the foreground window covers the whole monitor." On that monitor it hides the bars and BAND and stops underlay presents. It pauses input counting. The sim keeps running.

### 5.6 Input pulses: no way to see keys
- Raw Input with `RIDEV_INPUTSINK` on a message-only window on its own thread. Not a hook; hooks are banned in this codebase.
- **Keyboard: header only.** Key identity never enters BEZEL's memory.
- Mouse: buttons, wheel ticks, travel distance.
- `PulseBatch` counts are bucketed at 100 ms. They're never persisted and never sent over the network.
- On by default (D5), with the first-run card, tray toggle and Steam-page disclosure.
- D6 weights. A per-second wheel cap goes in the sim's saturation curve, because free-spinning wheels can produce thousands of ticks.
- Audit rule in CLAUDE.md: the keyboard path in `pulse.rs` may only read the header.
- Signed binaries. Scan with Defender and submit false positives before Fest.

---

## 6. Process and frame model

```
 input thread          Raw Input sink ───────────────────── PulseBatch ──┐
 shell-watch thread    global WinEvent hooks ─────────────── OsEvent ────┤
 settings thread       egui panel (only while open) ──────── Settings ───┤  crossbeam
 main thread           bars + BAND + underlays + AppBar callbacks + tray │
                       MsgWaitForMultipleObjectsEx ◄─────────────────────┘
                         drain → occlusion.apply() → CellDelta(cause)
                         → sim.step() (20 Hz) → layout (when dirty) → extract()
                         → renderer.submit(): bars = dirty rects + ONE Commit(); underlays = partial presents
                         → pacer
```

**Pacer (D9 + D10):**

| Tier | Rate | When |
|---|---|---|
| Ambient (floor) | 24 fps | always, whenever anything is visible |
| Active | 30 fps | units working, battle |
| Burst | 60 fps cap | crushes, snaps, parachutes, boss cores |
| Hidden | 0 | stand-down, session locked, display off, or a monitor whose underlay is fully covered |

**Sim:** fixed 20 Hz tick, seeded RNG, no wall-clock reads, render interpolation, per-build determinism (D13). **No offline progress (D34):** the sim runs only while BEZEL runs. It saves on exit and every 60 s.

**Audio (D35):** silent by default. Opt-in event sounds arrive after the fest, behind an `audio` port whose default is a null implementation.

**Zone isolation:** TRENCH, SHAFT, CANOPY and **FRONTIER** never call each other. They talk only through `economy::Ledger`, which carries resources, cargo in transit and salvage.

**Rendering (D8):** a sprite pass (aliased, nearest-neighbor sampling) followed by a geometry pass (antialiased ropes and FX) on every dirty region. Underlay regions first repaint their wallpaper underneath.

---

## 7. Workspace blueprint

```
bezel/
├── Cargo.toml                  # [workspace]; `windows` pinned to ONE version
├── rust-toolchain.toml
├── .cargo/config.toml
├── CLAUDE.md                   # invariants §2.7, dependency rule, input audit rule, THOR lease rule
├── docs/adr/0001-runtime-stack.md
├── assets/
│   ├── atlas/                  # sheets + .ron frame maps (hot-reload)
│   └── tuning/                 # trench canopy shaft frontier economy surfaces .ron (hot-reload)
├── crates/
│   ├── core/                   # POD types, ids, geometry, events. No deps.
│   ├── physics/                # rope verlet, grapple, falls, platform landing
│   ├── layout/                 # pure: zones, canopy modes, occlusion grid, platforms, fallback canopy rects
│   │   └── src/{lib,zones,canopy,occlusion,platforms,fallback,policy}.rs
│   ├── sim/
│   │   └── src/
│   │       ├── lib.rs zone.rs economy.rs config.rs save.rs
│   │       ├── canopy/{mod,scavenger,grapple,wallrun,band,rig}.rs
│   │       ├── frontier/{mod,structures,crush,bury,salvage,platforms,caches,refinery,defense,breach}.rs
│   │       ├── trench/{mod,foundry,terrarium,hatch}.rs
│   │       └── shaft/{mod,elevator,strata,battle}.rs
│   ├── render-api/             # DrawList, Renderer trait, atlas desc, SurfaceKind
│   ├── render-d2d/             # cfg(windows): DComp bars, underlay presenter, sprite + geometry passes, DWrite
│   │   └── src/{lib,device,bars,underlay,sprites,geometry,text,wallpaper}.rs
│   ├── render-null/            # golden-test recorder
│   ├── os-api/                 # port traits
│   ├── os-win32/               # cfg(windows): ALL unsafe Win32 code
│   │   ├── src/{lib,dpi,surfaces,appbar,desktop,wallpaper,winevent,pulse,fullscreen,tray,session}.rs
│   │   └── examples/           # spikes S0–S10
│   ├── os-fake/                # scripted multi-monitor window layouts + drags, pulses, dock, desktop layer
│   ├── steam/                  # PlatformServices: real at M1, null for tests
│   ├── ui-settings/            # egui panel, on-demand, own thread; includes the live "what BEZEL can see" panel (D36)
│   └── app/                    # composition root
│       └── src/{main,wiring,frame,pacer,extract,hotreload,diorama,firstrun}.rs
└── tools/
    ├── simctl/                 # headless balance runs (crush/salvage sweeps included)
    └── replay/                 # OsEvent + PulseBatch traces (counts only; window rects, no titles)
```

**Dependency rule:**
```
core ← physics ← sim
core ← layout ← sim          (D42)
core ← render-api ← { render-d2d, render-null }
core ← os-api     ← { os-win32,   os-fake }
core + layout + os-api ← ui-settings
app ← everything
```
Only `os-win32` and `render-d2d` may depend directly on `windows`. CI fails if `cargo tree -p bezel-sim -p bezel-layout -p bezel-physics` contains `windows`.

**Diorama mode** (`bezel-app --diorama`) shows a scaled virtual desktop with three fake monitors in one normal window. `os-fake` supplies draggable fake windows. It's the main way to iterate on crush and bury, platforms and canopy modes without wrecking the real desktop.

**Privacy note for traces:** `replay` records window *rects and ids only*. It never records titles, class names or process names.

---

## 8. Interface signatures

[UNTESTED: design contract, not compiled code.]

```rust
// ── crates/core ──────────────────────────────────────────────
pub type Px = i32;                                  // physical pixels, always

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectPx { pub l: Px, pub t: Px, pub r: Px, pub b: Px }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct MonitorId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub struct ExtWindowId(pub u64); // opaque; HWND never leaves os-win32
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct Tick(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId { pub monitor: MonitorId, pub cx: u16, pub cy: u16 }

pub const SIM_HZ: u32 = 20;
pub const DROP_POOL: u8 = 4;                        // fallback canopy only

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceId {
    Trench, Shaft, Underlay(MonitorId), Band, Diorama,
    Ledge, LaneL, LaneR, Drop(u8),                  // fallback canopy (S9 fail) only
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Edge { Left, Top, Right, Bottom }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Side { Left, Right }

/// Counts only. By design, no field can hold which key was pressed.
#[derive(Clone, Copy, Debug, Default)]
pub struct PulseBatch {
    pub bucket_ms: u16,
    pub key_events: u16,
    pub mouse_clicks: u16,
    pub wheel_ticks: u16,
    pub mouse_travel_px: u32,
}

#[derive(Clone, Debug)]
pub struct MonitorInfo { pub id: MonitorId, pub bounds: RectPx, pub work: RectPx, pub dpi: u32, pub primary: bool }

/// A window's top edge where the cells above it are bare (D24). In core so physics needn't depend on layout (D42).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform { pub window: ExtWindowId, pub monitor: MonitorId, pub y: Px, pub x0: Px, pub x1: Px }

#[derive(Clone, Copy, Debug)]
pub struct TargetWindow { pub id: ExtWindowId, pub frame: RectPx, pub monitor: MonitorId, pub maximized: bool }

/// Why a window's rect changed. The sim maps causes to crush / bury / unbury (D21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectCause { DragStart, DragSample, DragEnd, Shown, Hidden, Jumped, Minimized, Restored, Cloaked, Uncloaked, Destroyed }

#[derive(Clone, Debug)]
pub enum OsEvent {
    WindowRect { id: ExtWindowId, rect: Option<RectPx>, cause: RectCause },   // every tracked top-level window
    Foreground(Option<TargetWindow>),
    DisplaysChanged(Vec<MonitorInfo>),
    WallpaperChanged(MonitorId),
    DesktopLayerReset,                               // Explorer restart / underlay torn down → reattach
    Docked { surface: SurfaceId, rect: RectPx },
    StandDown { monitor: MonitorId, on: bool },
    SessionActive(bool),
}

// ── crates/os-api ────────────────────────────────────────────
use crossbeam_channel::Sender;

pub trait InputPulseSource: Send {
    fn start(&mut self, tx: Sender<PulseBatch>) -> Result<(), OsError>;
    fn set_paused(&self, paused: bool);
    fn stop(&mut self);
}

pub trait WindowTracker: Send {
    /// Global, out-of-context. Emits WindowRect hints for every tracked top-level window.
    fn start(&mut self, tx: Sender<OsEvent>) -> Result<(), OsError>;
    fn frame_of(&self, id: ExtWindowId) -> Option<RectPx>;    // DWM extended frame bounds; ≤ once per frame per dirty window
    fn snapshot(&self, m: MonitorId) -> Vec<(ExtWindowId, RectPx)>;  // full rescan: boot, foreground change, reattach
    fn stop(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum DockMode { Reserve, Yield }

pub trait ShellDock {
    fn monitors(&self) -> Vec<MonitorInfo>;
    fn taskbar_edge(&self, m: MonitorId) -> Edge;
    fn dock(&mut self, s: SurfaceId, m: MonitorId, edge: Edge, thickness: Px, mode: DockMode) -> Result<RectPx, OsError>;
    fn undock_all(&mut self);                        // every exit path
    /// Phase 2: the bar's window, for the renderer.
    fn surface_handle(&self, s: SurfaceId) -> Option<raw_window_handle::RawWindowHandle>;
    /// Phase 2 (S3): re-docks after ABN_POSCHANGED / Explorer restart; emits Docked on rect changes and StandDown on
    /// ABN_FULLSCREENAPP (bars hide, invariant 3). Call ~once a frame.
    fn service(&mut self) -> Vec<OsEvent>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopTopology { Legacy, Raised, Unavailable }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperFit { Fill, Fit, Stretch, Tile, Center, Span }

#[derive(Clone, Debug)]
pub struct WallpaperDesc { pub path: Option<std::path::PathBuf>, pub fit: WallpaperFit, pub background: [u8; 4] }

pub trait DesktopLayer {
    fn topology(&self) -> DesktopTopology;
    /// Create this monitor's underlay window in the desktop layer, beneath the icons.
    fn attach(&mut self, m: MonitorId, rect: RectPx) -> Result<raw_window_handle::RawWindowHandle, OsError>;
    fn detach_all(&mut self);                        // every exit path (invariant 6)
    /// Read-only. BEZEL never writes the wallpaper (invariant 6).
    fn wallpaper(&self, m: MonitorId) -> Option<WallpaperDesc>;
    fn foreign_occupant(&self, m: MonitorId) -> bool;    // live-wallpaper app or desktop widget detected
    fn set_mode(&mut self, m: MonitorId, mode: UnderlayMode) -> Result<(), OsError>;
    /// Phase 2 (S9): the layer itself notices Explorer restarts, outside teardown and z-order drift. ~1 Hz.
    fn health(&mut self) -> LayerHealth;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerHealth { Ok, ZOrderFixed, ResetNeeded }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlayMode { Opaque, TransparentOverForeign, Excluded }   // D38

pub trait FullscreenGuard { fn fullscreen_foreground(&self, m: MonitorId) -> bool; }

bitflags::bitflags! {
    pub struct SurfaceFlags: u8 { const INTERACTIVE = 1; const CLICK_THROUGH = 2; }
}

#[derive(Clone, Copy, Debug)]
pub enum PointerEvent {
    Down { s: SurfaceId, x: Px, y: Px },
    Up   { s: SurfaceId, x: Px, y: Px },
    Move { s: SurfaceId, x: Px, y: Px },
    Leave(SurfaceId),
}

pub trait SurfaceHost {
    fn create(&mut self, s: SurfaceId, rect: RectPx, flags: SurfaceFlags) -> Result<(), OsError>;
    fn set_rect(&mut self, s: SurfaceId, rect: RectPx);   // events only (invariant 2)
    fn set_visible(&mut self, s: SurfaceId, on: bool);
    fn native_handle(&self, s: SurfaceId) -> raw_window_handle::RawWindowHandle;
    fn drain_pointer(&mut self) -> Vec<PointerEvent>;
    fn cursor_near(&self, rect: RectPx, margin: Px) -> bool;   // BAND slide-away; sampled only on mouse pulses
}

// ── crates/render-api ────────────────────────────────────────
#[derive(Clone, Copy, Debug)] pub struct SpriteId(pub u32);
#[derive(Clone, Copy, Debug)] pub struct Rgba8(pub [u8; 4]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind { BarComposited, Underlay, Topmost }   // backend picks presenter per kind

#[derive(Clone, Debug)]
pub enum DrawCmd {
    Sprite    { id: SpriteId, frame: u16, x: f32, y: f32, flip_x: bool, alpha: u8 },
    Polyline  { pts: Vec<[f32; 2]>, width: f32, color: Rgba8, glow: bool },
    Particles { pts: Vec<[f32; 3]>, color: Rgba8 },
    Fill      { rect: [f32; 4], color: Rgba8 },
    Text      { s: String, x: f32, y: f32, size: f32, color: Rgba8 },
}

pub struct DrawList { pub surface: SurfaceId, pub dirty: Vec<RectPx>, pub cmds: Vec<DrawCmd> }
pub struct FrameStats { pub surfaces_drawn: u8, pub cpu_us: u32 }

pub trait Renderer {
    /// Phase 2: takes the surface's screen rect, not just its size (span wallpapers and screen-space draws need the origin).
    fn attach(&mut self, s: SurfaceId, kind: SurfaceKind, h: raw_window_handle::RawWindowHandle, rect: RectPx, art_scale: u8) -> Result<(), RenderError>;
    fn resize(&mut self, s: SurfaceId, size: (u32, u32));
    fn detach(&mut self, s: SurfaceId);
    fn load_atlas(&mut self, atlas: &AtlasDesc) -> Result<(), RenderError>;
    fn set_wallpaper(&mut self, m: MonitorId, image: Option<&std::path::Path>, fit: WallpaperFitR, background: Rgba8) -> Result<(), RenderError>;
    /// Phase 2 (D38): opaque, or transparent over a foreign live wallpaper. Needs the DComp presenter.
    fn set_underlay_transparent(&mut self, m: MonitorId, transparent: bool) -> Result<(), RenderError>;
    /// Bars: dirty rects + ONE atomic commit. Underlays: wallpaper repaint under dirty rects + partial present.
    fn submit(&mut self, frame: &[DrawList]) -> Result<FrameStats, RenderError>;
}

// ── crates/physics ───────────────────────────────────────────
#[derive(Clone, Copy, Debug)] pub struct RopeId(pub u32);

pub struct RopeWorld { /* verlet chains, fixed dt */ }
impl RopeWorld {
    pub fn spawn(&mut self, anchor: [f32; 2], end: [f32; 2], segments: u8) -> RopeId;
    pub fn pin_end(&mut self, r: RopeId, at: [f32; 2]);
    pub fn cut(&mut self, r: RopeId);
    pub fn step(&mut self, dt: f32);
    pub fn nodes(&self, r: RopeId) -> &[[f32; 2]];
}

pub struct FallBody { pub pos: [f32; 2], pub vel: [f32; 2], pub chute: bool }
pub enum Landing { Platform(ExtWindowId), Bedrock, StillFalling }
/// Falls through bare cells; lands on the first platform below (D24) or on bedrock at `bedrock_y` (the trench top).
/// `bedrock_y` added in phase 2: the original signature had no way to know where bedrock is.
pub fn fall_step(b: &mut FallBody, dt: f32, platforms: &[Platform], bedrock_y: Px) -> Landing;

// ── crates/layout ────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanopyPolicy { StrictExterior, EdgeHug }   // default EdgeHug (D1)

// Platform lives in core (D42); layout produces it, physics lands on it.

pub struct CellDelta { pub cause: RectCause, pub window: ExtWindowId, pub covered: Vec<CellId>, pub bared: Vec<CellId> }

pub struct OcclusionGrid { /* per-monitor u16 coverage counts + per-window last rect (u8 could wrap at 256 stacked windows) */ }
impl OcclusionGrid {
    pub fn new(monitors: &[MonitorInfo], cell_art_px: u16, art_scale: u8) -> Self;
    /// Incremental: −1 on old cells, +1 on new cells. Returns only cells that changed state (bare ↔ covered).
    pub fn apply(&mut self, id: ExtWindowId, new: Option<RectPx>, cause: RectCause) -> CellDelta;
    /// Union of every cell crossed between successive DragSample rects (for sweep-crush).
    pub fn sweep(&self, from: RectPx, to: RectPx) -> Vec<CellId>;
    pub fn is_bare(&self, c: CellId) -> bool;
    pub fn platforms(&self, m: MonitorId) -> Vec<Platform>;
    pub fn rebuild(&mut self, m: MonitorId, windows: &[(ExtWindowId, RectPx)]);   // boot / reattach / rescan
}

pub struct LayoutInput<'a> {
    pub home: &'a MonitorInfo,
    pub monitors: &'a [MonitorInfo],
    pub frontier_on: &'a [MonitorId],     // per-monitor toggle (D4 amended)
    pub taskbar: Edge,
    pub shaft_side: Side,                 // Left (D3)
    pub dock: DockMode,                   // Reserve (D2)
    pub sizes: &'a SurfaceSizes,
    pub art_scale: u8,
    pub target: Option<TargetWindow>,
    pub policy: CanopyPolicy,
    pub occlusion: &'a OcclusionGrid,
}

pub enum CanopyState {
    Frontier { outpost: Option<RectPx>, rope_l: Option<RectPx>, rope_r: Option<RectPx> },  // on the underlay, bare cells only
    WallRun { hook: [Px; 2] },
    Band { band: RectPx, hook: [Px; 2] },
    Dormant,
}

pub struct Zones { pub trench: RectPx, pub shaft: RectPx, pub underlays: Vec<(MonitorId, RectPx)>, pub canopy: CanopyState }

pub fn compute(input: &LayoutInput) -> Zones;
pub fn art_scale_for(dpi: u32) -> u8;

// ── crates/sim ───────────────────────────────────────────────
pub struct FrameInputs<'a> {
    pub pulses: PulseBatch,
    pub os: &'a [OsEvent],
    pub cells: &'a [CellDelta],           // cause-tagged; sim applies D21
    pub zones: &'a Zones,
    pub ui: &'a [UiCommand],
}

#[derive(Clone, Debug)]
pub enum SimEvent {
    RopeSnapped(RopeId),
    UnitFalling { unit: u32 },
    UnitLanded { unit: u32, on: LandingKind },
    StructureCrushed { id: u32, salvage: u32 },      // D25: 60% of build cost
    StructureBuried { id: u32 },
    StructureUnburied { id: u32 },
    RigDeployed, RigPacked,
    CargoDelivered { kind: Cargo, qty: u32 },
    BossCore { tier: u8 },
    BreachOpened { at: CellId },                     // D31, post-fest
    MonsterDown { id: u32, by: KillSource },         // turret, miner, or a crushing drag
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimDemand { Ambient, Active, Burst }        // no Static tier (D10)

pub trait Zone {
    fn step(&mut self, ctx: &mut ZoneCtx);
    fn anim_demand(&self) -> AnimDemand;
}

pub struct Sim { /* canopy, frontier, trench, shaft: Box<dyn Zone>; ledger; rng; tick */ }
impl Sim {
    pub fn new(cfg: SimConfig, seed: u64) -> Self;
    pub fn step(&mut self, inp: &FrameInputs) -> Vec<SimEvent>;
    pub fn snapshot(&self) -> SimSnapshot;
    pub fn anim_demand(&self) -> AnimDemand;            // no catch_up: no offline progress (D34)
    pub fn reload_config(&mut self, cfg: SimConfig);
    pub fn save(&self) -> SaveBlob;
    pub fn load(blob: &SaveBlob) -> Result<Self, SaveError>;
}

// ── crates/steam ─────────────────────────────────────────────
pub trait PlatformServices {
    fn init(&mut self) -> Result<(), PlatformError>;   // dev: Valve's generic test app id until BEZEL's own exists (D41) [VERIFY]
    fn run_callbacks(&mut self);
    fn unlock(&mut self, achievement: &str);
    fn cloud_write(&mut self, name: &str, data: &[u8]) -> Result<(), PlatformError>;
    fn cloud_read(&mut self, name: &str) -> Option<Vec<u8>>;
}

// ── crates/ui-settings ───────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Settings {
    pub dock: DockMode,
    pub home: MonitorId,
    pub frontier_on: Vec<MonitorId>,
    pub shaft_side: Side,
    pub canopy: CanopyPolicy,
    pub pulses_on: bool,
    pub audio_on: bool,                   // D35: false by default
}

pub trait SettingsPanel {
    fn open(&mut self, current: &Settings) -> Result<(), UiError>;   // focusable egui window, own thread, destroyed on close
    fn is_open(&self) -> bool;
    fn drain_changes(&mut self) -> Vec<Settings>;
    fn push_seen(&mut self, seen: &SeenReadout);                     // D36 live readout, only while open
    fn close(&mut self);
}

/// Exactly what BEZEL observes, shown live in settings (D36). No titles, no key identity.
pub struct SeenReadout {
    pub pulses_last_10s: PulseBatch,
    pub windows: Vec<RectPx>,
    pub wallpapers: Vec<(MonitorId, Option<std::path::PathBuf>)>,
}

// ── crates/app ───────────────────────────────────────────────
pub struct Ports {
    pub pulses:   Box<dyn InputPulseSource>,
    pub tracker:  Box<dyn WindowTracker>,
    pub dock:     Box<dyn ShellDock>,
    pub desktop:  Box<dyn DesktopLayer>,
    pub guard:    Box<dyn FullscreenGuard>,
    pub surfaces: Box<dyn SurfaceHost>,
    pub renderer: Box<dyn Renderer>,
    pub platform: Box<dyn PlatformServices>,
    pub settings: Box<dyn SettingsPanel>,
}

pub fn wire_win32()   -> Result<Ports, BootError>;
pub fn wire_diorama() -> Result<Ports, BootError>;
pub fn run(ports: Ports, sim: Sim) -> Result<(), RunError>;
```

---

## 9. Spikes: proof before M1

Each spike is a small binary in `os-win32/examples/`. Record pass/fail here.

**Batch 1** (gates the frontier, docking and Steam):

| # | Question | Pass | Fallback |
|---|---|---|---|
| **S9** | Desktop underlay on THOR (Windows 11, current build): detect legacy vs raised structure, attach per monitor beneath the icons; DirectComposition target vs flip-model swap chain on the child window; Explorer restart; wallpaper change; virtual-desktop switch; mixed-DPI monitors; **transparent mode over Wallpaper Engine / Lively** (Knox's top monitor) | Renders beneath windows and icons; icons visible and clickable; recovers from restarts and wallpaper changes; transparent mode shows the live wallpaper underneath with no flicker across reattach; the real wallpaper is untouched on exit | If the whole layer fails: rev-2 topmost canopy (LEDGE/LANES/DROP). If only transparent mode fails: live-wallpaper monitors excluded by default (D38) |
| **S10** | Occlusion fidelity: global WinEvent volume after filtering; extended-frame-bounds accuracy (no buried strip from invisible borders); other apps' transparent overlay windows ignored; drag-sweep sampling at ≤ 30 Hz; grid state matches a full rescan after a 10-minute session of heavy window use | Noise under N events/s; zero false-covered cells versus rescan; sweep catches a fast fling | Periodic full rescan (1 Hz) as a backstop |
| S3 | Two AppBars, bottom + left, with the Windows 11 bottom taskbar; Explorer restart; process killed with force | Work area correct; re-registers; no leaked reservation | Yield, shaft left |
| S0 | Two bar targets, one device, one `Commit()`, sprite crossing the corner at 60 fps | No tear | Corner owned entirely by TRENCH |
| S7 | Steamworks crate alongside DirectComposition; check overlay behavior | Achievements and cloud saves work | No overlay is acceptable |

**Batch 2:**

| # | Question | Pass | Fallback |
|---|---|---|---|
| S1 | BAND (and the fallback set) as a `NOREDIRECTIONBITMAP` DirectComposition window, fully click-through via `WS_EX_LAYERED\|WS_EX_TRANSPARENT` | Renders and passes clicks | `UpdateLayeredWindow` for BAND |
| S2 | `SetCapture` drag from a NOACTIVATE window that has never been activated | Tracks past the edge | Drags stay inside the surface |
| S4 | Footprint at the D10 floor with three underlays + bars, measured with PresentMon/ETW + Task Manager | Under 5% GPU on the RTX 3080 12GB; under 150 MB RAM; VRAM under 250 MB [UNTESTED targets] | Coarser cells, lower particle budget, underlay presents capped at 24 fps |
| S5 | Events from elevated windows | Delivered, or covered by the rescan backstop | Dormant on elevated targets |
| S6 | VRR: G-Sync game in borderless mode with BEZEL running | Stand-down fires; independent flip holds | More fullscreen heuristics |
| S8 | egui panel opened on demand on its own thread, 50 open/close cycles | No residue; no message-loop conflict | Panel in a child process |

### Results

**S9: PASS on the gate question (underlay, not the topmost fallback). Recovery items reported by Knox, not yet in a log.** Run 2026-11-07 ~20:00 on THOR, Windows build 10.0.26300.9818, binary `spike_s9_underlay` (debug).
- *Logged:* structure detected as **Raised** (Progman has `WS_EX_NOREDIRECTIONBITMAP`; icons are a child of Progman; shell wallpaper `WorkerW` under it). Three monitors, all 96 DPI: 1440×2560 portrait at (−1440,−389), primary 2560×1440 at (0,0), 2560×1440 at (1360,−1440). **DirectComposition target on a `WS_CHILD` of Progman worked on all three monitors**; the flip-swap-chain fallback wasn't needed. Attach took 92 ms.
- *Screenshot:* the checkerboard renders beneath app windows and beneath the desktop icons. Transparent mode over the live wallpaper (top monitor) shows the animated wallpaper through the checkerboard with the moving square on top. 22 fps against a 30 fps target (debug build, three monitor-sized swap chains, unpaced presents; not a perf number, S4 measures that).
- *Read-only probe after the run:* the live-wallpaper app puts one window tree per monitor (4 windows each) **inside the shell's wallpaper `WorkerW`**, which sits below our underlays under the raised structure. Occupancy detection found exactly those, so all three monitors correctly started in transparent mode (Knox runs a live wallpaper on every monitor). No underlay window was left under Progman after the process ended.
- *Reported by Knox, not in the log:* "everything seemed to work correct", without per-item detail. Unconfirmed checklist items: icons clickable, Explorer restart, wallpaper change, virtual-desktop switch, no flicker across reattach, clean exit. The log has no lines for any of these and no clean-exit line. The first build of the spike truncated its log on each start (now fixed: append), so a second launch may have erased them. **Explorer restart, wallpaper change and opaque mode: waived by Knox 2026-11-07, untested.** The recovery code exists in the spike; it gets exercised again in W6 hardening and at the demo gate (fest plan §4), which requires Explorer-restart and wallpaper-change recovery.
- *Re-run, 20:43, log fixed to append:* **5 forced reattaches (`r`) at 79–91 ms each,** every time Raised + DComp + transparent on all three monitors, 0 z-order drift fixes. **Clean quit:** "detached 3 underlay(s) (quit)", wallpaper never written. The first run's tail also shows a clean quit at 38 s. Still not in any log: an Explorer restart (no `TaskbarCreated` line), a wallpaper change, `t` (opaque mode). A virtual-desktop switch leaves no line when nothing breaks, so its silence counts as weak evidence it held. Flicker across a reattach rests on Knox's eyes.
- *Not testable on THOR:* mixed-DPI monitors (all three are 100%). Opaque mode wasn't exercised, because every monitor has a live wallpaper; press `t` to check it.
- *Consequence for D38:* under the raised structure, transparent mode works because the foreign wallpaper is *below* the underlay by construction (inside the shell `WorkerW`). The legacy structure hasn't been seen on THOR.

**S10: PASS.** Run 2026-11-07, 20:12–20:43 on THOR (1,847 s of Knox's normal use, three monitors at 96 DPI), binary `spike_s10_occlusion` (debug), log `spike_s10.log`.
- *Event volume:* 111,877 raw WinEvents (60.6/s average, peak 818/s); **2,455 kept by the filter** (top-level, `OBJID_WINDOW`/`CHILDID_SELF`): 1.3/s average, **peak 93/s**, above 20/s in 33 of 1,798 seconds. The filter keeps 2.2%. The ADR's "noise under N events/s" never had a number. Proposed N = 100 filtered/s peak, which this run meets.
- *Fidelity:* **364 rescans compared against the incremental grid, 0 mismatches: 0 false-covered, 0 false-bare cells,** final rescan included. 4 rescans were skipped because a drag was in progress. 7–13 windows counted at a time; 14 cloaked windows (other virtual desktops, hidden system UI) excluded at boot.
- *Drag sweep:* 329 drag samples at ≤ 30 Hz. The largest single-sample jump was **1,400 px** (a fast fling). The sweep interpolates at one cell per step, so that leaves no gap (unit-tested in `bezel-layout`).
- *Overlays:* at one point 2 click-through windows were up and were excluded. No window covering ≥ 90% of a monitor was topmost or layered; every such coverer was an ordinary maximized window.
- *Frame bounds:* 5 windows carry invisible resize borders outside their DWM frame (21 px average, left + right + bottom). The grid uses the DWM extended frame. The rescan uses the same source, so it can't catch a wrong rect source; the absence of a buried strip rests on Knox's eyes on the debug grid.
- *Finding for the sim (not a defect):* `sweep(from, to)` returns every cell the window touched along the path, including the cells it already covered. The sim has to subtract the window's own previous cells before crushing. That's why "cells swept" in the log runs ~7,000 per sample for a large window.
- *Not measured:* elevated windows (S5). The 1 Hz rescan backstop isn't needed on this evidence.

**S3: PASS.** Run 2026-11-07 22:01 on THOR by `tools/s3-harness.ps1` (no hands; work area read from a separate process), binary `spike_s3_appbars` (debug), logs `spike_s3.log` + `s3-harness.log`. 5/5 checks.
- *Docking:* TRENCH registered first at (0,1288)–(2560,1400), 2560×112, **above the Windows 11 taskbar**. SHAFT at (0,0)–(144,1288), stopping at TRENCH's top, so the corner belongs to TRENCH (§2.5). Primary work area went from (0,0,2560,1400) to **(144,0,2560,1288)**; the other monitors were untouched. One `ABN_POSCHANGED` right after docking; re-docking was idempotent.
- *Normal exit:* `ABM_REMOVE` on both; the work area was back to baseline within 0.6 s.
- *Explorer restart:* the reservation dropped when Explorer died (t=4.7 s), `TaskbarCreated` arrived at 7.2 s, both bars re-registered, and **the work area was restored at 8.9 s**. Maximized windows have ~4 s at full size in between; acceptable.
- *Force-kill (`Stop-Process -Force`, no cleanup ran):* **no leak. The shell reclaimed the space 1 s after the process died** (it notices the AppBar window is gone). The worst-bug case in CLAUDE.md doesn't happen on this build; `ABM_REMOVE` on every exit path stays mandatory anyway.
- *Final work area equals baseline.*
- *Not exercised:* `ABN_FULLSCREENAPP` (needs a fullscreen app; S6 covers stand-down), DPI change, home-monitor change.

**S0: PASS.** Run 2026-11-07 22:32 on THOR by Knox, binary `spike_s0_corner` (debug), log `spike_s0.log`.
- *Setup:* TRENCH (0,1288)–(960,1400) and SHAFT (0,648)–(144,1288) as plain topmost DComp popups (not AppBars) on **one DirectComposition device, each drawing into an `IDCompositionSurface`, both updated by one `Commit()` per frame**. A 56 px sprite crossed the seam at y=1288 at 700 px/s.
- *Knox's eyes:* **no tear.** The sprite's halves never split at the seam.
- *Pacing (informational; the GPU wasn't quiet, CAGED jobs were running):* paced by `DwmFlush` to the compositor, so it ran at the ~165 Hz refresh, not 60: 3,017 frames in 20 s (~157 fps), median interval 6.2 ms, **max `Commit()` 2.6 ms**, 119 frames (3.9%) over 1.5× the median, 0 draw errors.
- *Consequence:* the bars can share one device and one commit as §2.5 planned; the corner fallback (corner owned entirely by TRENCH) isn't needed. The real bars must pace to the D10 tiers, not to `DwmFlush`.

THOR: S4 and S6 are timed GPU work → quiet GPU plus a gpu lease, per the `thor_lease.ps1` header. Check status before release builds.

---

## 10. Consequences

**Easier:**
- The frontier gets occlusion for free.
- The catastrophe mechanic, platforms and bury/unbury all fall out of one incremental grid.
- Most click-through risk goes away; only BAND is topmost over app windows.
- Scrollbars are never covered.
- Maximized windows still get the canopy (wall-run).
- Cross-bar animation can't tear.
- Most code tests headless.
- Zones and renderer stay swappable.

**Harder:**
- **Dependence on an undocumented desktop structure** that Windows has already reshuffled once. That means two attach paths, recovery on Explorer churn, and a strict rule against ever touching the wallpaper.
- Global window tracking means noisy event streams and filters that have to be right.
- Three monitor-sized presenters under an always-on floor.
- Coexisting with live-wallpaper apps means two apps sharing one undocumented layer (D38: transparent mode, or exclusion as the fallback).
- Permanent work-area cost of the bars (112 px of height, 144 px of width on the home monitor).
- Pulses on by default puts the burden on the disclosure copy.
- No visual editor.
- Unsafe Win32 code that can't be reviewed by eye.

**Revisit:**
- Clickable frontier (post-fest).
- Default underlay mode per monitor, after S9.
- Crush salvage and wheel saturation after `simctl` sweeps.
- BAND distance and alpha after the first playtest.
- Spanning the bedrock across monitors (ADR-0003).

---

## 11. Action items
1. [ ] **Knox, W1:** gather the Steamworks paperwork in advance (ID, tax, bank) without paying anything. At the trailer-ready gate (D39), paying the fee should be the only step left.
2. [ ] **Knox, W5:** once the app exists, look up the exact January opt-in date for Desktop Companion Fest in Steamworks.
3. [ ] Set up the workspace per §7. Add `CLAUDE.md` with invariants 1–6, the dependency rule, the input audit rule, the trace-privacy rule and the THOR lease rule.
4. [ ] Spike batch 1: **S9 and S10 first**, then S3 and S0. S7 only if the demo ships.
5. [ ] Build the pure crates `core`, `layout` and `physics`, with tests.
6. [ ] Work through `docs/plan/fest-2027.md` week by week.

---

## Sources
- Steamworks, Upcoming Steam Events (Desktop Companion Fest listing): https://partner.steamgames.com/doc/marketing/upcoming_events
- Steamworks, Steam Themed Sale Events (eligibility, tags): https://partner.steamgames.com/doc/marketing/upcoming_events/themed_sales
- Valve's first-half 2027 schedule (coverage): https://www.digitalcitizen.life/valve-reveals-full-steam-sale-and-festival-schedule-through-july-2027/
- FeatherWall architecture notes on the Windows 11 24H2+ raised desktop structure: https://raw.githubusercontent.com/RitvikDayal/featherwall/main/docs/architecture.md
- Drawing behind desktop icons (legacy WorkerW technique): https://codeproject.com/Messages/5096131/Thanks
- Live-wallpaper failure mode in the wild (covers icons/desktop): https://steamcommunity.com/app/2003310/discussions/0/4406292251590833992
- Kenny Kerr, "High-Performance Window Layering Using the Windows Composition Engine," MSDN Magazine: https://msdn.microsoft.com/en-us/magazine/dn745861.aspx
- Raymond Chen, "Like the cake, WS_EX_TRANSPARENT is a lie, or at least not the entire truth": https://devblogs.microsoft.com/oldnewthing/20121217-00/?p=5823
- Partial click-through on topmost layered/DComp windows (field report): https://windows-hexerror.linestarve.com/q/so55202379-how-to-properly-allow-click-through-areas-of-transparent-sections-of-topmost-window
- Godot passthrough region also clips drawing on Windows (4.4 thread): https://forum.godotengine.org/t/how-can-i-make-a-transparent-app-on-windows/110395
- Godot 4.3 transparency sometimes renders black: https://forum.godotengine.org/t/window-transparency-sometimes-not-working/86812
- Godot 4.4.1 desktop-pet transparency/passthrough glitches: https://forum.godotengine.org/t/need-help-visual-glitch-of-windows-tool-bar-and-dark-screen-using-transparent-and-passthrough-windows/116526
