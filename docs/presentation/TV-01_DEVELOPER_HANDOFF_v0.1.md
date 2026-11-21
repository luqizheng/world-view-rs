# World View — TV-01 Developer Handoff

- Version: V0.1
- Date: 2026-11-21
- Status: Ready for implementation
- Work type: Focused technical validation prototype; not production renderer

## 1. Purpose

Build a small runnable desktop prototype to validate the proposed World View presentation technology boundary:

- C# / .NET 10 and Avalonia 12 for the desktop shell.
- Skia for custom map-viewport drawing.
- One text-shaping authority: Avalonia's configured shaping pipeline (HarfBuzz where required by the selected Avalonia 12 renderer configuration); Skia is used for glyph drawing, not as a separate shaping engine.
- An immutable, renderer-neutral Scene Snapshot as the renderer's only scene input.
- Clear separation among UI interaction, background preparation, and render callback execution.

This prototype validates feasibility. It must not become a parallel implementation of Core Domain.

## 2. Project constraints

1. The target desktop route is C# + Avalonia 12 + .NET 10.
2. The previous Rust + Bevy implementation is historical reference only; do not port its renderer by default.
3. Core Domain / Domain Services remain authoritative for formal map data, history, geometry rules, and commands.
4. Presentation subsystems prepare immutable presentation results; the renderer only draws them.
5. The render callback must not query Core Domain, persistence, ViewModels, or mutable UI state.
6. Keep the prototype small and maintainable. Do not build a plugin framework or split into many projects without a demonstrated need.

## 3. Scope

### In scope

- Desktop shell with one map viewport.
- Avalonia custom draw operation backed by Skia.
- Renderer-neutral immutable scene model.
- Rendering of points, straight polylines, closed polygon paths, and cubic Bézier curves.
- Basic pan, zoom, resize, clipping, and DPI conversion.
- Text validation for Latin, CJK, mixed Latin/CJK, and RTL samples.
- Background creation and atomic publication of complete immutable snapshots.
- Diagnostics for snapshot version, viewport dimensions, DPI, and basic frame timing.
- Windows and macOS build/run instructions and validation report.

### Out of scope

Production editing tools, Core Domain/database integration, production CRS/projection services, GIS ingestion, terrain/raster rendering, spatial indexes, hit testing, snapping, full animation, historical reconstruction, narration, complete label placement/collision avoidance, print/layout workspace, collaboration, and production performance claims.

## 4. Suggested project structure

A single solution and one desktop project are acceptable. Use folders/namespaces to express boundaries:

```text
WorldView.PresentationPrototype/
  WorldView.PresentationPrototype.sln
  src/WorldView.PresentationPrototype/
    App.axaml
    App.axaml.cs
    Program.cs
    MainWindow.axaml
    MainWindow.axaml.cs
    Rendering/
      MapViewport.cs
      MapDrawOperation.cs
      SkiaSceneRenderer.cs
    Scene/
      SceneSnapshot.cs
      ScenePrimitive.cs
      ScenePublisher.cs
    Viewport/
      ViewportTransform.cs
      ViewportState.cs
    Diagnostics/
      RenderDiagnostics.cs
  tests/WorldView.PresentationPrototype.Tests/
  README.md
  TV-01-validation-report.md
```

Do not create production libraries just to make the prototype look enterprise-grade.

## 5. Required contracts

### 5.1 Immutable Scene Snapshot

Each snapshot must be immutable after publication and carry at least:

- monotonically increasing SnapshotVersion;
- diagnostic creation timestamp;
- immutable collection of scene primitives;
- optional text/glyph presentation data required by the selected Avalonia drawing route.

The production presentation context will additionally carry Map ID, Branch ID, canonical Historical Time, WorldState revision/fingerprint, projection definition/version, style revision, viewport generation, and context fingerprint. TV-01 may use a compact test context but must demonstrate atomic publication and version consistency.

A draw operation captures exactly one snapshot reference and uses it for the entire draw. It must never fetch geometry, labels, or poses independently during rendering.

### 5.2 Scene primitives

Keep primitives backend-neutral:

- PointPrimitive
- PolylinePrimitive
- PolygonPrimitive
- CubicBezierPrimitive
- TextLabelPrimitive (only as needed for text validation)

Coordinates must be explicitly identified as map/test coordinates, never device pixels.

### 5.3 Coordinate pipeline

Use explicit stages:

```text
Map/Test coordinates
  -> Viewport logical pixels
  -> Device physical pixels (backend / DPI boundary)
```

No implicit mixing of coordinate spaces. Pan/zoom use an explicit viewport transform. DPI scaling is applied exactly once. This prototype has no production CRS service; do not claim geographic distance or scale correctness.

### 5.4 Atomic publication

Build complete snapshots away from the render path. Publish the immutable reference atomically. A draw already in progress finishes with its captured snapshot; the next draw sees the new snapshot. Never mutate collections after publication. Cancellation must not publish partial data.

## 6. Avalonia / Skia guidance

1. Use Avalonia's supported custom drawing extension point for the viewport (`ICustomDrawOperation`, after verifying the exact Avalonia 12 API).
2. Keep custom drawing limited to the map viewport; do not move the whole application UI into Skia.
3. Acquire and dispose Skia drawing leases/resources within their documented callback lifetime.
4. Verify exact API signatures, package compatibility, and lease lifetime against Avalonia 12 documentation and installed package metadata; do not copy obsolete samples.
5. Set correct operation bounds. Its hit-test behavior is only coarse scene-graph behavior; real map hit testing belongs to Spatial Presentation and is outside TV-01.
6. Never retain SKCanvas, drawing context, or lease objects between callbacks.
7. Select one text-shaping authority. If explicit Skia rendering requires Avalonia.HarfBuzz / UseHarfBuzz configuration, configure and test it. Skia drawing itself is not a text-shaping engine.
8. Do not introduce a second independent shaping pipeline; layout metrics must correspond to the glyph run ultimately drawn.
9. Pin exact package versions and record them in README.

If the exact Avalonia 12 API differs from this guidance, follow the documented API and record the deviation.

## 7. Threading and resource lifetime

| Area | Allowed | Prohibited |
|---|---|---|
| Avalonia UI thread | Controls, input, viewport interaction state, scheduling | Long-running geometry/layout work |
| Background worker | Build immutable scene data and test calculations | Direct mutation of UI controls |
| Render callback | Read captured immutable snapshot and issue drawing commands | Database/domain access, ViewModel access, blocking waits, domain writes |

Render work must be bounded. Shutdown must stop outstanding background work and release resources safely. Never solve cross-thread access by blocking the render callback or synchronously dispatching to UI.

## 8. Validation plan

- **TV-01 Desktop baseline:** build and launch on Windows and macOS; record OS, SDK, packages, and commands.
- **TV-02 Custom viewport:** draw all required primitive types; resize repeatedly; verify bounds, clipping, and resource lifetime.
- **TV-03 Text:** test Latin, CJK, mixed scripts, RTL, punctuation, combining marks, and font fallback; confirm shaping and drawing metrics agree.
- **TV-04 Coordinate/DPI:** pan/zoom and test 100%, 125%, 150%, and 200% scaling where available; verify map-space anchors remain correct.
- **TV-05 Snapshot/threading:** publish snapshots repeatedly from a background worker with visible version markers; verify each frame contains one version only; test cancellation and shutdown.
- **TV-06 Performance observations:** use synthetic fixtures of 1,000 / 10,000 / 50,000 primitives; record frame time, CPU/memory observations, scene-build time, machine, and display. These are test fixtures, not product promises.
- **TV-07 Terrain seam:** no terrain rendering required; only show that a future raster/tile presentation layer can be represented without embedding terrain storage/GIS semantics in the renderer. Do not choose a canonical terrain format here.

## 9. Acceptance criteria

TV-01 is accepted only when:

1. The solution builds with documented .NET 10 SDK and pinned dependencies.
2. It launches on Windows and macOS, or any limitation is documented with reproducible evidence.
3. Points, straight lines, polygons, and cubic Bézier curves render.
4. Pan, zoom, resize, and DPI behavior are demonstrated.
5. Latin, CJK, mixed, and RTL text samples are validated; shaping authority and limitations are documented.
6. Snapshots are immutable after publication and atomically replaced.
7. A draw uses one captured snapshot version only.
8. Render callback code accesses no UI/ViewModel state, Core Domain, or persistence.
9. Drawing resources are released within their documented lifetime.
10. Automated tests and manual validation results are recorded.
11. README includes exact Windows/macOS setup, build, run, and test instructions.
12. No approved architecture boundary is weakened to simplify the prototype.

## 10. Deliverables

Required: source solution, tests for snapshot publication and viewport transforms, README with pinned package versions and commands, and `TV-01-validation-report.md` containing environment, test results, screenshots/recordings where useful, performance observations, known limitations, deviations, and a recommendation (proceed / proceed with conditions / revise technology choice).

Optional: packaged builds and a short architecture diagram.

## 11. Implementation sequence

1. Verify .NET 10 SDK and Avalonia 12 package compatibility.
2. Create the minimal desktop shell and prove it starts on both target platforms.
3. Implement one custom draw operation and render one primitive.
4. Add immutable scene model and atomic publisher.
5. Add remaining primitives.
6. Add pan/zoom and DPI diagnostics.
7. Validate text shaping and glyph drawing.
8. Add background publication and concurrency tests.
9. Run synthetic performance fixtures.
10. Submit the validation report for architecture review.

Do not start with terrain, animation, full label placement, or Core Domain integration.

## 12. Risks and guardrails

- **API drift:** verify against Avalonia 12, not older examples.
- **Text metric mismatch:** one shaping authority only.
- **DPI double-scaling:** document logical-to-physical conversion explicitly.
- **Mutable scene leakage:** immutable data after publication.
- **Render callback overreach:** no database, domain, UI state, or blocking calls.
- **Premature abstraction:** validate only required boundaries.
- **False production confidence:** TV-01 does not validate production CRS, spatial indexing, animation, terrain, or production-scale performance.

## 13. Developer kickoff note

Before implementation, report the selected .NET SDK and exact Avalonia/Skia/HarfBuzz package versions, the documented Avalonia 12 custom-drawing API path, the text-shaping configuration, availability of Windows/macOS build environments, and any blocking incompatibility discovered.

Ordinary implementation details do not require product decisions. Escalate only if a discovered issue materially changes the approved architecture or platform commitment.

## 14. Validation status at handoff

This document is an implementation brief, not a claim that the prototype is built.

At handoff, architecture compatibility has been reviewed for the available Spatial Presentation, Animation Runtime, and Text & Label contracts; Avalonia custom drawing and text-shaping configuration have been reviewed from documentation. Compilation, Windows/macOS runtime, DPI, Unicode, threading, performance, and terrain-seam validation remain pending.

The separate Renderer Contract & Scene Model V0.1 was not available in the working directory when this handoff was assembled. Do not claim it is included; retrieve and reconcile it before production renderer implementation.
