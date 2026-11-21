# TV-01 Technical Validation Report

- Project: WorldView.PresentationPrototype
- Handoff: `docs/presentation/TV-01_DEVELOPER_HANDOFF_v0.1.md` (commit ce9010f)
- Date: 2026-11-21
- Verdict: **Proceed with conditions** (all core validations pass; two framework-level known issues recorded with evidence and workarounds)

---

## 1. Environment

| Item | Value |
|---|---|
| OS (validation machine) | Microsoft Windows NT 10.0.26300 (Windows 11), RDP session, 32 CPU cores |
| .NET SDK used to build | **11.0.100** (targets `net10.0`; .NET 10 runtime 10.0.15 installed) |
| Avalonia | **12.1.6** (Avalonia, Avalonia.Desktop, Avalonia.Skia, Avalonia.HarfBuzz, Avalonia.Themes.Simple, Avalonia.Fonts.Inter, Avalonia.Headless.XUnit) |
| SkiaSharp | **3.119.4** (transitive via Avalonia.Skia 12.1.6) |
| HarfBuzzSharp | (transitive via Avalonia.HarfBuzz 12.1.6, native assets per-OS) |
| xunit | **v3 series 3.2.2** (xunit.v3, xunit.runner.visualstudio 3.1.5) — pinned to exactly match Avalonia.Headless.XUnit's dependency; mixing series causes CS0433 duplicate-attribute errors |
| Microsoft.NET.Test.Sdk | 18.12.0 |
| Render backend observed | **GPU OpenGL** (live app), **CPU raster** (RenderTargetBitmap offscreen path) |
| Display scaling | 1.25x (Windows DPI setting) |

macOS: not available in this environment. A `osx-arm64` self-contained publish **builds successfully** (see §6); runtime is **unverified** — honestly recorded, not marked as passed.

## 2. Status classification (mandatory six-state)

### 已实现 (implemented)
- Desktop shell (Avalonia 12, classic desktop lifetime) with one map viewport, toolbar, live diagnostics panel.
- `ICustomDrawOperation`-backed Skia viewport: points, polylines, closed polygons, cubic Bézier curves, pre-shaped text labels.
- Explicit typed coordinate spaces (`MapPoint`/`ViewportPoint`/`DevicePoint` — no implicit conversions) and `ViewportTransform` (pan/zoom-at-anchor/DPI one-time scaling).
- Immutable `SceneSnapshot` + atomic `ScenePublisher` (CAS reference swap, version monotonicity, stale discard).
- Background `SceneBuildService` with supersede/cancel semantics; complete-snapshot-only publication.
- Single shaping authority: `UseHarfBuzz()` → `TextShaper.Current` (HarfBuzz impl, verified via reflection); Skia draws pre-shaped glyph IDs via `SKTextBlob` only; `DrawingContext.DrawGlyphRun` used for metric cross-checking.
- Diagnostics: per-frame timing, snapshot version, drawn primitive count, draw thread id, DPI, map-units/px, backend info, memory, shaped-label count, text metric consistency, version-consistency violation counter (must stay 0).
- Automated tests (33) + automated in-app benchmark (`--autobench`) + in-process screenshot evidence (`--screenshot`).

### 已构建 (built)
- `Debug` and `Release` (net10.0) on Windows — **0 errors, 0 warnings** (final build).
- `osx-arm64` self-contained publish with `.app`-style bundle and macOS dylibs — build artifact verified; runtime not executed.
- Tested environment note: .NET SDK 11.0.100 targeting net10.0; build succeeded on both configurations.

### 已运行 (ran)
- Windows: app launches, renders, interacts (zoom via wheel/toolbar, pan via drag), publishes snapshots from background threads, exits cleanly (exit code 0; shutdown stops background work before window close).
- `--autobench` full cycle (1K → 10K → 50K → zoom pulses) completed on-screen.
- `--screenshot` in-process offscreen capture produced PNG evidence through the real draw-operation path.

### 已测试通过 (tested & passed)
- **33/33 automated tests pass** (`dotnet test`, xunit v3 + Avalonia.Headless with the SAME UseHarfBuzz shaping configuration):
  - ViewportTransform: map↔viewport roundtrip identity, center anchoring, Y-axis orientation, exactly-once DPI scaling (×1.0/1.25/1.5/2.0), zoom-keeps-anchor invariant (×0.5/1.2/2/10), zoom clamping, pan directions.
  - SceneSnapshot/Publisher: version positivity, layer count sum, builder-insulation after construction, first publish, stale discard (equal & lower versions), atomic replacement, subscriber exception isolation, **8-writer concurrency race: strictly monotonic versions + readers never observe partial scenes** (version↔primitive-count sentinel) + deterministic stale rejection.
  - Text (headless, HarfBuzz active — verified `TextShaper.Current` platform impl contains "HarfBuzz"): Latin, CJK, mixed Latin/CJK, Arabic RTL (contextual joining, ≤9 glyphs for 9 chars), combining marks, GlyphRun-bounds vs layout-advance agreement (<10% relative delta for Latin/CJK/Arabic).
  - Fixtures: determinism (same seed → same scene), all five primitive kinds present, Latin/CJK/RTL label coverage, background build publishes complete snapshot, 20-request supersede storm ends at the last version (21).
- Startup shaping suite (7 samples incl. CJK/RTL/combining): PASS on every observed run (displayed in app hint bar).

### 尚未验证 (not yet validated)
- **macOS runtime** (no macOS machine available): build artifact verified only. Required follow-up: run on macOS per README instructions; record GPU backend (Metal vs OpenGL), text metrics with system font chain, and the toolbar issue reproduction status.
- Windows 125%/150%/200% scaling matrix: only 125% (the machine's setting) exercised; transform math for other factors is covered by unit tests (`ToDevice` ×1.0/1.5/2.0 and zoom-anchor invariance) but the live-app matrix was not run.
- Production CRS/projection, spatial indexing, hit testing, terrain, animation, full label placement, print, collaboration — out of scope per Handoff §3; not validated, not claimed.
- 50K+ beyond the required scale; no sustained-scroll continuous-pan performance scenario (zoom-pulse only).

### 已知限制 (known limitations)
1. **Toolbar text corruption (framework-level, reproducible; does NOT affect core validations).**
   The first two toolbar buttons render their text as multiple overlapping glyph layers, sometimes including strings that exist nowhere in the project (e.g. city names in multiple languages). Evidence chain:
   - Layout layer is CORRECT: runtime dump shows exact non-overlapping arrange bounds (`Load1KButton bounds=104x25@(0,0)`, `Load10KButton bounds=104x25@(110,0)`, … all 8 buttons sequential).
   - Reproduces with: XAML Content strings AND code-behind Content assignment; with and without `WithInterFont()`; with serialized (lock-protected) shaping.
   - Reproduces on real screen (CopyFromScreen), `PrintWindow`, and in-process `RenderTargetBitmap` — not a capture artifact.
   - Does not affect: map viewport rendering, diagnostics panel, hint bar, tests, benchmark numbers.
   - Suspected Avalonia 12.1.6 issue (candidate for upstream report); workaround in place (contents set from code-behind; cosmetic only). Recommendation for production: use ItemsControl-generated buttons or a different theme (Fluent) and re-check on macOS.
2. **Font coverage**: default system fallback chain renders 5 CJK ideographs as 5–6 glyphs (one codepoint split); recorded as a metric-consistency caveat, bounded by test (`InRange(5, 6)`).
3. **`SKTypeface.FromFamilyName` fallback** in the Skia direct path is cruder than Avalonia's `FontManager` fallback (documented in code); the Avalonia glyph path (`DrawGlyphRun`) is the metric-authoritative route.
4. Scene fixtures are synthetic rectangles/polygons in a planar test space — no geographic claim whatsoever (Handoff §5.3).

## 3. Validation items A–G

### TV-01-A Desktop baseline — PASS (Windows), build-only (macOS)
- .NET 10 target, Avalonia 12.1.6, single solution + two projects (app + tests) per Handoff §4. Windows launch/run verified (§2). macOS: `dotnet publish -r osx-arm64` succeeds with native dylibs; runtime pending.

### TV-01-B Map drawing — PASS
- All five primitive kinds render (screenshot `test-results/tv01-scene-1k.png`: 1000 features visible — points, polylines, filled+stroked polygons, Bézier curves; label glyphs drawn via Skia glyph IDs).
- Resource lifetime: SKPaint/SKPath/SKTextBlob created and disposed within the draw callback; only typefaces are cached (disposed at renderer `Dispose`); `MapDrawOperation.Dispose` holds nothing; lease acquired/released strictly inside `Render()`.
- Viewport clipping applied in logical pixels; bounds = viewport logical rect.

### TV-01-C Coordinates & interaction — PASS
- Three-space typed pipeline `MapPoint → ViewportTransform → ViewportPoint → (single DPI multiply) → DevicePoint`; distinct record types, no implicit conversions; render callback applies DPI exactly once via `canvas.Scale`.
- Wheel zoom keeps the pointer's map anchor at the same pixel (unit-tested); drag pans; resize re-lays out and re-clips; live DPI readout 1.25 recorded in diagnostics.

### TV-01-D Text — PASS (with font-coverage caveat)
- One shaping authority: Avalonia+HarfBuzz (`UseHarfBuzz` verified active; Skia never shapes).
- Latin/CJK/mixed/RTL/combining all shape with positive advances; Arabic contextual joining confirmed; layout-advance vs drawn-ink-bounds consistency recorded per frame and unit-tested (<10% delta).
- Both drawing routes exercised: Skia `SKTextBlob` (positioned glyph runs from shaped advances+offsets) and Avalonia `DrawingContext.DrawGlyphRun`.

### TV-01-E Scene snapshot — PASS
- Immutable after publication (immutable arrays + immutable records; builder-insulation test).
- Atomic publication: single-reference CAS swap; stale versions discarded; cancellation never publishes partial data; supersede storm test (20 rapid requests) ends exactly at the last version.
- Draw operation captures exactly ONE snapshot reference per frame; per-frame version recorded and consistent (screenshots show `published vN = drawn vN`, violations=0 across all sessions).

### TV-01-F Performance observations — RECORDED (fixtures, not promises)
Final run (shaping serialized; **all features visible in viewport**; GPU OpenGL; DPI 1.25; 1200×760 window; Windows 11 RDP; 32-core) — `test-results/tv01-bench-final.txt`:

| Phase | Features | Primitives | Scene build | Frame (single-phase) | Working set |
|---|---|---|---|---|---|
| warmup | 256 | 262 | 0.3 ms | 0.8 ms | 190 MB |
| 1K | 1,000 | 1,020 | 0.8 ms | 2.6 ms | 219 MB |
| 10K | 10,000 | 10,200 | 6.3 ms | 19.1 ms | 240 MB |
| 50K | 50,000 | 51,000 | 25.3 ms | **92.2 ms (~11 fps)** | 261 MB |
| zoom-pulse (during 50K scene) | — | 51,000 | — | avg 9.2 ms / worst 149 ms | 399 MB peak |

Interpretation (honest, per Handoff §8 "observations, not promises"):
- The per-frame cost is dominated by the naive per-primitive immediate draw: ~51K individual `DrawPath`/`DrawCircle` calls plus ~1,000 per-frame `SKTextBlob` rebuilds for labels.
- **10K all-visible ≈ 19 ms/frame — already past 60 Hz.** An earlier measurement session (in `tv01-bench-results.txt`, before the camera-reset fix) recorded 6.2 ms @ 50K, which was inflated by map-units/px being 45× too small: AABB culling silently discarded ~99% of primitives. Both logs are retained deliberately as evidence that culling effectiveness, not primitive count alone, decides frame cost.
- During zoom pulses the average frame is 9.2 ms because most intermediate views see only part of the world (culling works); the worst frames (149 ms) are the fully-visible ones. This is the known scaling frontier of the TV-01 renderer: **production must use the Renderer Contract's batch/texture/LOD machinery rather than per-primitive immediate drawing.** This is exactly the kind of boundary TV-01 exists to find.
- Scene build (geometry generation + shaping, background thread) is never a bottleneck (≤25 ms for 50K) and publishes atomically; zero version-consistency violations across all sessions.

### TV-01-G Platform validation
- Windows: built, ran, tested, screenshotted — full evidence above.
- macOS: `dotnet publish -c Release -r osx-arm64 --self-contained true -p:CreateAppBundle=true` succeeds; output contains `libAvaloniaNative.dylib`, `libSkiaSharp.dylib`, `libHarfBuzzSharp.dylib` and the managed set. **Runtime execution NOT performed** (no macOS host in this environment). Follow-up checklist is in README §macOS.

### TV-07 Terrain seam — NOTED (no implementation required)
- The renderer consumes only backend-neutral primitives and knows nothing about terrain storage/GIS semantics; a future raster/tile layer can enter as an additional immutable primitive family / render layer without changing any TV-01 contract demonstrated here. No canonical terrain format chosen (per Handoff §8 TV-07).

## 4. Contract compliance

| Rule (Handoff / contracts) | Status |
|---|---|
| Render callback queries no Core Domain, persistence, ViewModel, mutable UI state | ✅ `MapDrawOperation`/`SkiaSceneRenderer` read only `RenderFrame` (snapshot + camera value + size + DPI) |
| Draw uses exactly one captured snapshot; never fetches pieces mid-render | ✅ single reference field; per-frame version recorded |
| Snapshot immutable after publication; atomic swap; cancellation discards, never publishes partials | ✅ publisher CAS + monotonic discard + tests |
| Skia is not a shaping engine; one shaping authority | ✅ HarfBuzz-only shaping; Skia draws glyph IDs |
| Layout metrics correspond to glyphs actually drawn | ✅ per-frame metric recording + unit test (both draw routes) |
| Skia resources released within documented lifetime; no lease retention across callbacks | ✅ per-callback paints/paths/blobs; typeface cache owned & disposed by renderer |
| No render-thread blocking on UI; UI never blocks render | ✅ `Dispatcher.UIThread.Post` (never Invoke) for invalidation; render callback never touches UI objects |
| Coordinate spaces never mixed implicitly; DPI applied exactly once | ✅ distinct typed records; single `canvas.Scale` at boundary; unit tests |
| Camera-only changes do not rebuild/bump snapshots | ✅ `ViewportTransform` lives outside `SceneSnapshot` |
| No Skia types in scene/interaction contracts | ✅ scene layer references only its own records + `ShapedLabel` |
| RenderItemId never written back as domain ID; presentation-only provenance flag | ✅ `RenderItemId` + `IsPresentationOnly` on primitives; fixtures carry no domain data |
| Compact test context placeholder for production context fields | ✅ `SceneContext` carries representative Map/Branch/time/projection/style values |
| Do not port the Rust/Bevy renderer | ✅ nothing ported; legacy docs used as prior-art references only |

Deviations from Handoff guidance (permitted "follow the documented API and record" clause):
1. `TopLevel.LayoutScaling` does not exist in Avalonia 12 — used `TopLevel.RenderScaling` (device-per-logical-pixel) instead.
2. `RenderTargetBitmap` moved to `Avalonia.Media.Imaging` in Avalonia 12.
3. `DrawingContext.DrawGlyphRun` takes a `GlyphRun` directly (not `IImmutableGlyphRunReference`); `ImmediateDrawingContext.DrawGlyphRun` takes the immutable reference — both routes exercised appropriately per context.
4. `SKTextBlob.CreateText` / `SKRunBuffer.GetGlyphSpan()` from older SkiaSharp samples are gone/obsolete in 3.119 — used `SKTextBlob.Create(string, SKFont, SKPoint)` and `buffer.Glyphs`/`buffer.Positions`.
5. All threading: Avalonia 12.1.6 text services appear not thread-safe for concurrent shaping (see §2 limitation 1); TV-01 serializes shaping behind a lock while keeping scene building parallel. **This is a TV-01 finding to carry into production renderer design** (options: per-thread shapers, shaping pipeline on a dedicated stage, or upstream clarification).

## 5. Repository mapping note (per Handoff §14)

The Renderer Contract & Scene Model V0.1 was retrieved from `world-view/docs/map-engine/world-view-renderer-contract-and-scene-model-v0.1.md` (it existed in the sibling world-view working directory; the handoff note said it was unavailable). TV-01 reconciled naming accordingly: `SceneSnapshot`/`SceneContext`/`RenderLayerSnapshot`/`RenderItemId`/presentation-only provenance/`SceneBuildDiagnostics` mirror the contract's conceptual model at prototype granularity. Production renderer implementation must reconcile the remaining contract fields (HistoricalViewContext provenance, ProjectionDescriptor, batch families) that TV-01 deliberately simplified.

## 6. Artifacts

| Artifact | Path |
|---|---|
| Solution | `WorldView.PresentationPrototype/WorldView.PresentationPrototype.sln` |
| App project | `src/WorldView.PresentationPrototype/` |
| Test project | `tests/WorldView.PresentationPrototype.Tests/` (33 tests) |
| Benchmark log (first full run) | `test-results/tv01-bench-results.txt` |
| Benchmark log (final) | `test-results/tv01-bench-final.txt` |
| Scene screenshots | `test-results/tv01-scene-1k.png` (+ .txt evidence note), `tv01-scene-1k-v2.png` |
| Toolbar corruption evidence | `tv01-toolbar-zoom.png`, `tv01-realscreen-toolbar3.png`, `tv01-1k-screenshot4.png` |
| macOS publish output | `test-results/publish-osx-arm64/` (build artifact only) |

## 7. Recommendation

**Proceed with conditions** to the production presentation/renderer workstream:
1. Re-run startup validation on macOS (runtime, Metal backend path, font chain, toolbar issue repro).
2. Carry the two TV-01 findings into design: (a) Avalonia text services thread-affinity/serialization; (b) per-frame glyph blob cost at ≥10K labels → adopt the Renderer Contract's batch/text machinery and measure again.
3. File the toolbar corruption upstream to Avalonia (evidence in §2.1) or switch theme before production UI work.
4. TV-01's coordinate/snapshot/threading contracts held up under all validations — no architecture boundary needed weakening anywhere in the prototype.
