# TV-01 Follow-up Experiments — Toolbar Text Corruption (Expert Review Item 3)

Date: 2026-11-21
Status: investigation in progress — root cause NOT yet confirmed; do not declare an Avalonia defect yet.
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
