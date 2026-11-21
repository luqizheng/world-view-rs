# WorldView.PresentationPrototype — TV-01

Avalonia 12 + Skia technical validation prototype for the World View presentation boundary.
Scope and acceptance criteria: `docs/presentation/TV-01_DEVELOPER_HANDOFF_v0.1.md` (in the repo root's docs folder).
Validation results: [TV-01_TECHNICAL_VALIDATION_REPORT.md](TV-01_TECHNICAL_VALIDATION_REPORT.md).

**This is a technical validation prototype.** It is not the production renderer, not a map editor, and contains no Core Domain data.

## Pinned toolchain

| Component | Version |
|---|---|
| Target framework | net10.0 |
| .NET SDK (used here) | 11.0.100 (compiles net10.0) |
| Avalonia / Skia / HarfBuzz / Themes.Simple / Fonts.Inter / Desktop / Headless.XUnit | 12.1.6 |
| SkiaSharp (transitive) | 3.119.4 |
| xunit.v3 (pinned to match Headless.XUnit) | 3.2.2 |
| xunit.runner.visualstudio | 3.1.5 |
| Microsoft.NET.Test.Sdk | 18.12.0 |

`NuGet.config` pins sources: nuget.org first, huaweicloud mirror as regional fallback (identical upstream packages).

## Build & run (Windows)

```bash
# from this folder
dotnet build WorldView.PresentationPrototype.sln -c Debug

# interactive app (toolbar: load 1K/10K/50K, zoom, reset; wheel = zoom at pointer, drag = pan)
cd src/WorldView.PresentationPrototype/bin/Debug/net10.0
./WorldView.PresentationPrototype.exe

# automated performance observation (1K/10K/50K + zoom pulses, writes a report file, closes itself)
./WorldView.PresentationPrototype.exe --autobench C:/path/to/tv01-bench-results.txt

# in-process scene screenshot through the real draw pipeline (works over headless RDP)
./WorldView.PresentationPrototype.exe --screenshot C:/path/to/scene.png 1000
```

If the window closes immediately, check `tv01-crash.log` next to the exe (WinExe has no console).

## Build & run (macOS) — runtime UNVERIFIED

A self-contained osx-arm64 bundle builds successfully on Windows. On a macOS (Apple Silicon) machine:

```bash
# rebuild natively (recommended; AppleSilicon SDK required)
dotnet publish src/WorldView.PresentationPrototype -c Release -r osx-arm64 --self-contained true -p:CreateAppBundle=true

# or copy test-results/publish-osx-arm64/ to the mac, then:
chmod +x WorldView.PresentationPrototype
./WorldView.PresentationPrototype

# first launch may need: xattr -dr com.apple.quarantine WorldView.PresentationPrototype.app (or the exe)
```

Record on macOS: rendering backend (Metal path), startup shaping suite result (hint bar), whether the toolbar-text corruption known issue reproduces, and one `--autobench` run.

## Tests

```bash
dotnet test WorldView.PresentationPrototype.sln
```

33 tests: viewport transform math (roundtrip/DPI-once/zoom-anchor/pan), snapshot immutability, atomic publisher (incl. 8-writer concurrency race + stale discard), headless HarfBuzz shaping (Latin/CJK/mixed/RTL/combining + metric agreement), deterministic fixtures, background build supersede/cancel.

Headless tests use `Avalonia.Headless.XUnit` with the same `UseHarfBuzz()` configuration as the app, so the shaping authority under test is identical.

## Project structure (boundaries per Handoff §4)

```text
src/WorldView.PresentationPrototype/
  Program.cs                  entry; UseHarfBuzz() = the single shaping-authority configuration
  MainWindow.axaml(.cs)       shell wiring only (toolbar/viewport/diagnostics, --autobench/--screenshot)
  Rendering/
    MapViewport.cs            UI control: camera state, input, per-frame snapshot capture
    MapDrawOperation.cs       ICustomDrawOperation: ONE captured snapshot per draw, lease lifetime, DPI-once
    SkiaSceneRenderer.cs      draws primitives + pre-shaped glyph IDs onto leased SKCanvas
    SceneBuildService.cs      background builds, supersede/cancel, complete-snapshot-only publication
    RenderFrame.cs            immutable per-frame input (snapshot + camera + size + DPI)
  Scene/
    SceneSnapshot.cs          immutable snapshot (version/context/layers/diagnostics)
    ScenePrimitives.cs        Point/Polyline/Polygon/CubicBezier/TextLabel primitives (map-space typed)
    ScenePublisher.cs         atomic publication (CAS swap, monotonic versions, stale discard)
    SceneFixtures.cs          deterministic synthetic scenes (1K/10K/50K), mixed-script labels
  Viewport/
    Coordinates.cs            MapPoint/MapRect/ViewportPoint/ViewportRect/DevicePoint/ColorRgba (no implicit conversion)
    ViewportTransform.cs      the ONLY map↔viewport converter (+ exactly-once DPI boundary)
    ViewportState.cs          UI-thread camera state
  Text/
    TextShapingService.cs     single shaping entry (serialized: Avalonia text services are not thread-safe)
    ShapedLabel.cs            immutable shaped payload (glyph IDs + advances + GlyphRun for cross-check)
  TextValidation.cs           startup shaping suite (Latin/CJK/mixed/RTL/combining)
  Diagnostics/RenderDiagnostics.cs  thread-safe counters polled by the UI
tests/WorldView.PresentationPrototype.Tests/
  TestAppBuilder.cs           headless app factory (UseHarfBuzz kept identical)
  ViewportTransformTests.cs   coordinate pipeline (13 cases)
  SceneSnapshotAndPublisherTests.cs  immutability + atomicity + concurrency (9 cases)
  TextShapingAndFixtureTests.cs      shaping + fixtures + background build (11 cases)
TV-01_TECHNICAL_VALIDATION_REPORT.md   results, evidence, known issues, recommendation
test-results/                    bench logs, screenshots, macOS publish output (not committed except logs/screenshots)
```

## Architectural rules enforced in code (do not weaken)

1. The render callback (`MapDrawOperation.Render` → `SkiaSceneRenderer.Draw`) reads ONLY its captured `RenderFrame`: no controls, no ViewModels, no domain, no persistence.
2. `SceneSnapshot` is immutable after publication; publication is a single atomic reference swap; stale versions are discarded.
3. One shaping authority: Avalonia + HarfBuzz. Skia draws pre-shaped glyph IDs; it never shapes text.
4. Coordinate spaces are distinct types; DPI scaling happens exactly once in `MapDrawOperation.Render`.
5. Camera state lives outside the snapshot (pan/zoom never rebuilds or re-versions scenes).
6. Skia resources are acquired and disposed within the draw callback; only typefaces are cached and disposed at shutdown.
