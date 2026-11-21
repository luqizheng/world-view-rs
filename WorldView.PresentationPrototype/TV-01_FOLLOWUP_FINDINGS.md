# TV-01 Follow-up Experiments — Toolbar Text Corruption (Expert Review Item 3)

Date: 2026-11-21
Status: investigation in progress — root cause NOT yet confirmed; do not declare an Avalonia defect yet.
Status update (2026-11-21, console machine): **console session reproduces the corruption**, so RDP is
NOT the trigger. Bisection narrowed the minimal trigger to `DrawingContext.DrawGlyphRun` on the
hand-built `GlyphRun` in `MapViewport.DrawGlyphRunSamples`; Skia custom draw, background publishing,
theme, renderer backend and session type are all excluded. See "Console session results" below.
Baseline: TV-01 commit `7b40861`, Avalonia 12.1.6, Windows 11 (26300), **RDP session**, DPI 1.25.

Symptom (main prototype): the FIRST TWO toolbar buttons ("Load 1K", "Load 10K") render their
text as several overlapping glyph layers, sometimes including strings that exist nowhere in the
project (city names in multiple languages). Buttons 3..8 render correctly. Layout layer verified
correct via runtime bounds dump (`Load1KButton bounds=104x25@(0,0)`, `Load10KButton @(110,0)` —
no overlap). Map viewport, diagnostics panel, hint bar are unaffected.

## Experiments run so far

| # | Condition | Result | Evidence |
|---|---|---|---|
| 1 | SimpleTheme (baseline) | corrupted (first 2 buttons) | `test-results/tv01-scene-1k-v7.png` |
| 2 | FluentTheme (`--theme-fluent`) | **still corrupted** → not theme-specific | `test-results/tv01-fluent-toolbar.png` |
| 3 | Inter font removed (system default) | still corrupted → not Inter-specific | earlier session |
| 4 | Shaping serialized behind global lock | still corrupted → not (only) shaping race | earlier session |
| 5 | XAML Content vs code-behind Content | both corrupted → not the XAML string path | earlier session |
| 6 | **Minimal repro project** (`repro/ToolbarTextRepro`, 6 buttons, SimpleTheme + UseHarfBuzz, NO Skia custom draw) | **CLEAN — does NOT reproduce** | `test-results/tv01-repro-harfbuzz.png` |
| 7 | In-process RenderTargetBitmap capture (no screen/RDP pipeline involved) | corrupted → not an RDP capture artifact | `tv01-scene-1k-v7.png` |
| 8 | Software rendering (`--software`, CPU renderer) | **still corrupted** (run under RDP) | `test-results/tv01-software-toolbar.png` |
| 9 | Real screen (CopyFromScreen) | corrupted → same as offscreen captures | `tv01-realscreen-toolbar3.png` |
| 10 | **Console session, GPU (OpenGL)**, main prototype | **still corrupted** → RDP is not the trigger | `test-results/console-gpu-toolbar.png` |
| 11 | Console session, `--theme-fluent` | still corrupted | `test-results/console-fluent-toolbar.png` |
| 12 | Console session, `--software` (CPU raster) | still corrupted | `test-results/console-software-toolbar.png` |
| 13 | Console session, minimal repro (`repro/ToolbarTextRepro`) | **CLEAN** | `test-results/console-repro.png` |
| 14 | Console, bisect A: no background publishing, 64-feature scene published synchronously on UI thread | still corrupted → background build thread not required | `test-results/console-bisect-a-no-background.png` |
| 15 | Console, bisect B: `context.Custom(MapDrawOperation)` removed | still corrupted → **Skia custom draw exonerated** | `test-results/console-bisect-b-no-customdraw.png` |
| 16 | Console, bisect C: `MapViewport` not hosted (toolbar-only window) | **CLEAN** → trigger lives in `MapViewport` render path | `test-results/console-bisect-c-toolbar-only.png` |
| 17 | Console, bisect D: `DrawGlyphRunSamples(context)` removed (all else restored) | **CLEAN** → minimal trigger = `DrawingContext.DrawGlyphRun` | `test-results/console-bisect-d-no-drawglyphrun.png` |
| 18 | Console, bisect E: only Latin labels fed to `DrawGlyphRun` | still corrupted → not a cross-typeface/cross-script collision | `test-results/console-bisect-e-latin-only-drawglyphrun.png` |

## Interpretation so far

- The corruption is tied to the MAIN PROTOTYPE's environment, not to buttons/themes/fonts/shaping
  concurrency per se: the minimal repro with the same theme + HarfBuzz + identical button texts is clean.
- Since offscreen RenderTargetBitmap shows it too, RDP's display remoting is not the cause of the
  artifact itself. However, ALL experiments so far ran inside an RDP session, so RDP may still
  influence which render backend/texture path Avalonia uses (e.g. ANGLE on top of WARP).
- Primary remaining hypothesis: interaction between the Skia custom draw operation (GPU texture
  path) and UI text glyph resources inside the same window (glyph atlas / texture sharing), or a
  render-loop interaction (background snapshot publishing + continuous invalidation). The clean
  repro lacks: Skia custom draw, background build threads, DispatcherTimer-driven invalidation.
- NOTE: performance numbers in the validation report were also measured under RDP; re-baseline on
  a console session before comparing future numbers.

## Next steps (on a real console machine, NOT RDP)

1. `git pull` the repo; build and run the main prototype normally; inspect the toolbar.
2. If clean on console → the trigger involves the RDP/GPU-compositor environment; repeat `--software` and `--theme-fluent` under RDP-only comparisons and file the upstream issue with both session types documented.
3. If still corrupted on console → bisect by stripping the prototype: (a) remove SceneBuildService/background publishing, (b) remove the MapDrawOperation custom draw, (c) keep only the toolbar. Narrow until the repro project reproduces, then report upstream to Avalonia with the minimal project.
4. Run `dotnet test` (33 tests expected green) and, if a mac is available, the macOS checklist in README.
5. Record all findings in `test-results/` and commit.

## Repro project usage

```bash
cd WorldView.PresentationPrototype/repro/ToolbarTextRepro
dotnet run
# window shows 6 buttons; compare first two against test-results/tv01-repro-harfbuzz.png
# toggle UseHarfBuzz() in Program.cs to test the shaping configuration variable
```

---

## Console session results — RDP ruled out (2026-11-21)

### Environment

| Item | Value |
|---|---|
| OS | Windows 11 Enterprise, build 26300 |
| Session type | **`console`** — `query session` shows `>console  leo  1  Active`; no `rdp-tcp#N` session exists |
| DPI | 1.25 (window 1518×997 device px) |
| GPU | Intel Iris Xe + NVIDIA RTX 4060 Laptop; Avalonia backend `OpenGL` (default) / `CPU raster` (`--software`) |
| Caveat | An Oray virtual-display driver (`OrayIddDriver Device`) is installed; the OS session is still a local console session |
| Toolchain | .NET SDK 11.0.100, Avalonia 12.1.6 |
| Commit | `f9b3697` (main, includes `c2e2bfa`) |

### Per-mode results (real console session)

| Mode | Result | Evidence |
|---|---|---|
| Main prototype, GPU (OpenGL) | **corrupted** (first 2 buttons) | `test-results/console-gpu-toolbar.png` |
| `--theme-fluent` | corrupted | `test-results/console-fluent-toolbar.png` |
| `--software` (CPU raster) | corrupted | `test-results/console-software-toolbar.png` |
| Minimal repro `repro/ToolbarTextRepro` | **CLEAN** | `test-results/console-repro.png` |

`dotnet test`: **33/33 passed**.

### Bisection on the console machine

Each step is cumulative; source edits were reverted afterwards (no committed source change).

| Step | Change | Result |
|---|---|---|
| A | No background publishing; 64-feature scene built & published synchronously on the UI thread | still corrupted |
| B | A + `context.Custom(new MapDrawOperation(...))` removed (zero Skia drawing) | still corrupted |
| C | B + `MapViewport` not added to the visual tree (toolbar-only window) | **CLEAN** |
| D | Original code, **only** `DrawGlyphRunSamples(context)` removed | **CLEAN** |
| E | `DrawGlyphRunSamples` kept but fed **Latin/ASCII labels only** | still corrupted |

### Conclusion

- **RDP / GPU-compositor is not the trigger.** The corruption reproduces identically on a real
  console session, with GPU (OpenGL) and with CPU raster, and with both themes.
- **Minimal trigger:** `DrawingContext.DrawGlyphRun(glyphRun)` in `MapViewport.DrawGlyphRunSamples`,
  where `glyphRun` is the hand-built `GlyphRun` created in
  `TextShapingService.Shape` (`new GlyphRun(glyphTypeface, size, text.AsMemory(), glyphInfos,
  baselineOrigin, bidiLevel)`), executed every viewport frame.
- **Excluded variables:** session type (RDP vs console), theme (Simple/Fluent), Inter font,
  renderer backend (GPU vs CPU), Skia custom draw, background build threads, shaping-thread
  concurrency, and cross-typeface/cross-script mixing (Latin-only still corrupts).
- **Observed mechanism (not yet proven upstream):** drawing that hand-constructed `GlyphRun`
  poisons the glyph cache shared with the toolbar `TextBlock` rendering. The overlapping layers
  on the toolbar contain the exact multi-script scene-label strings ("city names") that only
  `DrawGlyphRunSamples` produces. The repro project is clean because it never calls
  `DrawGlyphRun` — it has no such labels.
- Why only buttons 1–2: they are the first text content laid out/drawn in the window, so their
  glyph-cache entries are the ones aliased by the samples. This is an observation, not a proven
  ordering guarantee.

### Upstream recommendation

1. File an Avalonia issue with a minimal project derived from this prototype: a window with a few
   `Button`s plus a `Control` whose `Render` calls `DrawingContext.DrawGlyphRun` with a `GlyphRun`
   built from `TextShaper.Current.ShapeText(...)` + `new GlyphRun(glyphTypeface, size,
   text.AsMemory(), glyphInfos, baselineOrigin, bidiLevel)`. Include
   `console-bisect-d-no-drawglyphrun.png` (clean) vs `console-gpu-toolbar.png` (corrupted).
2. Interim prototype workaround: remove `DrawGlyphRunSamples` from `MapViewport.Render` (metric
   cross-check is already covered by the headless shaping tests), or obtain the `GlyphRun` through
   Avalonia's `TextLayout` instead of constructing it by hand.
3. TV-01 expert-review item 3: close as "not RDP; trigger is the Avalonia `DrawGlyphRun` +
   hand-built `GlyphRun` path" and carry it into the upstream report.
4. macOS checklist: not run (no macOS machine available in this session).

---

## Experiment A/B/C + interim fix (2026-11-21)

Full report: `docs/presentation/TV-01_FOLLOWUP_TOOLBAR_TEXT_CORRUPTION.md`.

- **A (TextLayout GlyphRun):** drawing a `GlyphRun` obtained from Avalonia `TextLayout` through
  the same `DrawingContext.DrawGlyphRun` is **clean** (2/2 runs); drawing the hand-built
  `GlyphRun` from `TextShapingService.Shape` is corrupted (2/2 runs).
  Evidence: `test-results/console-exp-a-{manual,textlayout}-run{1,2}.png`.
- **B (construct vs consume):** shaping + `GlyphRun` construction WITHOUT drawing is **clean**;
  only the `DrawGlyphRun` consumption of the hand-built run corrupts.
  Evidence: `test-results/console-exp-b-construct-only.png`, `console-exp-b-off.png`.
- **C (standalone minimal project):** `repro/ToolbarTextRepro` with the same 20 mixed-script
  labels, same hand-built construction, code-behind button content, optional startup suite and
  optional delayed activation — **does NOT reproduce** in any combination. Therefore this is
  NOT declared an Avalonia framework defect.
  Evidence: `test-results/console-exp-c-*.png`.
- **Interim fix:** `DrawGlyphRunSamples` removed from `MapViewport.Render` (diagnostic code no
  longer in the production render path). Toolbar is clean in Debug and Release; 33/33 tests pass.
  Evidence: `test-results/console-after-fix-{debug,release}-toolbar.png`.
- Confidence: **Strongly Indicated** — trigger is confirmed inside the prototype; whether it is a
  framework defect remains unresolved because the standalone project cannot reproduce it.


