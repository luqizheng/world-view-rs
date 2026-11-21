# Production Renderer Architecture V0.2

- **Date:** 2026-11-21
- **Status:** Architecture baseline for production design and implementation planning
- **Target desktop stack:** C# / .NET 10 / Avalonia 12 / Skia
- **Supersedes:** TV-01 prototype-only assumptions where this document defines production behavior
- **Related evidence:** `TV-01_DEVELOPER_HANDOFF_v0.1.md`, `TV-01_FOLLOWUP_TOOLBAR_TEXT_CORRUPTION.md`
- **Scope:** Production presentation and rendering architecture; not a replacement for Core Domain contracts

---

## 0. Executive decision

World View production rendering is a dedicated presentation subsystem hosted by **Avalonia 12** and rendered through **Skia** for the map viewport.

The renderer is not the owner of map data, historical truth, geometry editing rules, animation history, terrain truth, or label semantics. It consumes one coherent, immutable presentation snapshot assembled from authoritative upstream services.

The production pipeline is:

```text
Core Domain / Domain Services
        │ authoritative World State at Map + Branch + Historical Time
        ▼
Presentation Context
        │ immutable identity for one coherent presentation evaluation
        ▼
Presentation Preparation
  ├─ Spatial Projection & Geometry
  ├─ Animation Runtime
  ├─ Text & Label Engine
  ├─ Map Decorations
  └─ Style / Symbol Resolution
        ▼
Scene Assembly
        │ one immutable Scene Snapshot
        ▼
Avalonia Map Viewport / Custom Draw Operation
        ▼
Skia Renderer Backend
        ▼
Display
```

The renderer may be re-evaluated at any time from the same Presentation Context. It must not accumulate historical state from previous frames.

### 0.1 Decisions established by V0.2

1. **Core Domain remains the only authority for formal map state.** Presentation code cannot write domain data or invent historical facts.
2. **One Presentation Context binds every asynchronous presentation result.** Geometry, animation pose, labels, decorations, and style must agree on the same context fingerprint.
3. **One immutable Scene Snapshot is atomically published.** A frame captures exactly one snapshot and never mixes versions.
4. **Avalonia owns the desktop shell and UI.** Skia is used for the map viewport drawing backend, not as a replacement for the entire UI framework.
5. **HarfBuzz remains the single text-shaping authority.** Skia draws shaped glyph IDs; Avalonia TextLayout is an allowed Avalonia drawing route only when it uses the configured HarfBuzz shaping path.
6. **Hand-constructed GlyphRun objects must never be passed to Avalonia DrawingContext drawing APIs.** This is a production implementation prohibition based on TV-01 evidence, not a claim that an Avalonia framework defect has been proven.
7. **Spatial operations, animation evaluation, text layout, and scene rendering are separate responsibilities.**
8. **Renderer callbacks are read-only, bounded, and non-blocking.** No Core Domain, database, filesystem, ViewModel, or mutable UI state access is permitted there.
9. **Terrain is an upstream environment dependency for movement planning and terrain presentation.** The renderer does not re-plan movement or modify historical data during playback.
10. **V1 production rendering prioritizes correctness, determinism, maintainability, and measured performance over premature GPU or framework abstraction.**

---

## 1. Goals and non-goals

### 1.1 Goals

- Render maps from authoritative World State for a selected Map, Branch, and Historical Time.
- Support projected geometry, points, lines, polygons, Bézier curves, Unit poses, labels, and map decorations.
- Support pan, zoom, viewport resize, DPI scaling, hit testing, snapping proposals, and editing previews through clearly separated presentation services.
- Keep the renderer backend replaceable without changing Core Domain semantics.
- Ensure the same presentation context yields deterministic presentation output, subject only to explicitly documented rasterization/platform differences.
- Support asynchronous preparation without stale or mixed-context results.
- Provide diagnostics and validation hooks for performance, correctness, and resource lifetime.

### 1.2 Non-goals

V0.2 does not define or implement:

- Core Domain persistence schemas or historical event semantics.
- A second geometry or topology authority inside the renderer.
- A second Unit movement history or animation truth source.
- GIS ingestion, canonical terrain storage format, or a complete projection catalogue.
- Collaborative rendering or remote rendering.
- A plugin framework for arbitrary render backends.
- A requirement to render every map feature as an individual draw call.
- A requirement to use Skia for ordinary application controls.
- An Avalonia framework defect diagnosis for the TV-01 toolbar issue.

---

## 2. Architectural boundaries

### 2.1 Core Domain and Domain Services

Core Domain / Domain Services own:

- Map, Object, Geometry Component, Event, Branch, Unit, and formal historical state.
- Historical ordering and state reconstruction.
- Geometry validity, topology rules, and formal geometry operations.
- Formal movement descriptors and their authoritative time semantics.
- Commands, validation, persistence, and revision identity.

Presentation code receives read-only, versioned data through supported application/domain interfaces. It must not query persistence directly or write domain state.

A presentation interaction may produce a **proposal** or an application-level intent. Only the relevant Domain Service may validate and commit a formal change.

### 2.2 Presentation subsystem

The Presentation subsystem converts authoritative state and view configuration into renderable output. It may cache derived data, but every cache is disposable and rebuildable.

Its principal services are:

| Service | Responsibility | Must not own |
|---|---|---|
| Presentation Context | Bind identity and version of one evaluation | Domain truth |
| Spatial Presentation | Projection, coordinate transforms, bounds, spatial index, hit/snap candidates, previews | Formal geometry edits |
| Animation Runtime | Deterministic Unit visual pose at a requested time | Movement history |
| Text & Label Engine | Shaping, candidate placement, collision, label layout | Textual historical facts |
| Decoration Layout | Legend, scale bar, north arrow, grid, frame, title, attribution | Map Object identity |
| Scene Assembler | Combine compatible immutable results | Domain rules |
| Renderer Backend | Draw the assembled scene | Queries, edits, or history |

### 2.3 Renderer

The renderer consumes only a complete immutable Scene Snapshot plus the backend resources needed to draw it.

It does not:
- Reconstruct historical state.
- Perform formal geometry operations.
- Decide whether a snap is valid.
- Decide whether a Unit can traverse terrain.
- Choose or modify historical movement routes.
- Generate label content from unrelated domain records.
- Read mutable controls, ViewModels, or application state from the render callback.

---

## 3. Unified Presentation Context

All presentation work for one displayed world must be associated with a single immutable Presentation Context.

### 3.1 Required identity

A Presentation Context contains at least:

| Field | Meaning |
|---|---|
| MapId | Map being presented |
| BranchId | Branch whose history is being evaluated |
| HistoricalTime | Canonical domain time, not a UI-formatted string |
| WorldStateRevision / Fingerprint | Identity of the authoritative World State |
| ProjectionDefinitionId + Version | Projection and coordinate interpretation |
| StyleRevision | Effective style/theme/symbol configuration |
| ViewportGeneration | Version of viewport dimensions, transform, and DPI |
| FontConfigurationRevision | Font set and fallback configuration identity |
| TerrainRevision / Dataset Fingerprint | Required when terrain-dependent output is present |
| ContextFingerprint | Stable fingerprint of all inputs that affect derived presentation output |

The exact serialization/hash algorithm is an implementation detail, but the fingerprint must be stable for equal inputs and must change whenever a presentation-relevant input changes.

### 3.2 Context consistency rule

Every asynchronous result carries the ContextFingerprint that produced it.

- Results with a fingerprint different from the current requested context are stale and must be discarded.
- Scene Assembly may combine only results with the same fingerprint.
- The published snapshot must expose the fingerprint and a monotonic snapshot version.
- No frame may combine geometry from one context with Unit poses or labels from another.

### 3.3 Viewport changes

Viewport changes do not change historical truth, but they do change presentation evaluation inputs. A viewport generation changes when relevant dimensions, logical-to-device scaling, camera transform, or clipping state changes.

Work started for an older viewport generation may finish, but its result must not be published into the newer generation unless the result is explicitly viewport-independent.

---

## 4. Coordinate and projection pipeline

### 4.1 Typed coordinate spaces

The implementation must make coordinate spaces explicit. At minimum:

1. **Domain coordinates:** authoritative geometry coordinates and their declared CRS.
2. **Map/projected coordinates:** coordinates after the selected projection or local mapping transform.
3. **Viewport logical pixels:** Avalonia logical pixel space.
4. **Device physical pixels:** backend/device pixel space after DPI scaling.

Conversions must be explicit and owned by named services. Do not pass untyped pairs of floating-point values between layers when the coordinate space is ambiguous.

### 4.2 CRS categories

V1 must distinguish at least:

- Geographic coordinates.
- Projected coordinates.
- Local Cartesian/map coordinates.

The system must not infer metric distances from geographic degrees or from arbitrary local coordinates. Scale bars and measurement tools must declare whether the selected CRS supports meaningful distance measurement.

### 4.3 Projection service

Spatial Presentation calls a projection service that declares:

- Source and target coordinate reference.
- Forward and inverse transform availability.
- Valid coordinate domain and failure behavior.
- Precision/tolerance policy.
- Version identity for cache keys and Presentation Context.

Projection failure must be explicit. Invalid or out-of-domain coordinates must not silently become (0,0).

### 4.4 DPI rule

DPI scaling is applied exactly once at the logical-pixel/device-pixel boundary. Hit and snap tolerances are defined in viewport logical pixels unless a tool explicitly documents otherwise.

---

## 5. Spatial Presentation and geometry interaction

### 5.1 Geometry representation

Presentation must preserve source geometry semantics. V1 supports:

- Points.
- Straight line segments and polylines.
- Closed polygon rings and polygon collections.
- Cubic Bézier curves.
- Mixed straight/Bézier paths where the source contract permits them.

Bézier flattening is a presentation approximation for tessellation, bounds, or broad-phase indexing only. It must not replace or overwrite the authoritative source curve.

### 5.2 Bounds and spatial indexing

- Bounds must be conservative: they may be larger than the exact geometry but must never exclude visible or hittable geometry.
- Broad-phase queries use an in-memory spatial index (R-tree, BVH, or another measured structure).
- Narrow-phase hit testing evaluates the actual geometry and current viewport transform.
- Indexes are derived caches and can be rebuilt from immutable geometry snapshots.
- V1 does not require SQLite R*Tree persistence.

### 5.3 Hit testing

Hit testing is a Spatial Presentation responsibility, not Avalonia scene-graph hit testing.

The pipeline is:

1. Convert pointer location to the appropriate typed coordinate space.
2. Query broad-phase candidates.
3. Evaluate precise geometry distance/intersection in viewport-aware tolerance.
4. Apply deterministic ranking and tie-breaking.
5. Return a read-only hit result with source identity and context fingerprint.

Avalonia control hit testing is used only for normal UI controls and coarse viewport routing.

### 5.4 Snapping

V1 snap candidate families:

- Grid.
- Geometry node/vertex.
- Geometry edge/segment.
- Intersection.
- Auxiliary/construction guide.

Candidate ranking must be deterministic. Candidate generation and ranking do not commit data.

A snap result is a **proposal** containing candidate identity, proposed coordinate, source context, and relevant tolerance. The Domain Service validates the resulting formal command before commit.

### 5.5 Drag and edit preview

- Dragging creates a presentation-only preview.
- The preview is separate from the committed Scene Snapshot.
- Preview updates must not mutate domain geometry.
- On commit, the interaction layer submits a formal command through the application/domain boundary.
- On cancel, stale context, or validation failure, the preview is discarded.
- A commit based on a stale WorldState revision must be rejected or revalidated by the authoritative layer.

---

## 6. Animation Runtime integration

### 6.1 Authority and responsibility

Timeline selects Historical Time. Core Domain reconstructs the authoritative World State. Animation Runtime evaluates visual Unit pose from the formal movement descriptor and the requested time.

Animation Runtime is a deterministic evaluator, not a second history store.

### 6.2 Deterministic evaluation

For a given Map, Branch, Historical Time, WorldState revision, movement descriptor revision, and relevant terrain/profile inputs, Unit pose evaluation must be deterministic.

- Any-time seek evaluates directly at the requested time.
- Do not compute pose by accumulating `previous_position + velocity × frame_delta`.
- Frame delta may schedule playback but must not define historical truth.
- Playback speed changes the UI clock scheduler only.
- Reverse playback changes the queried time, not the semantic direction of the Unit's orientation.
- Paused, resumed, forward, reverse, and seek operations must all converge to the same pose for the same canonical time.

### 6.3 Route and pose semantics

The Runtime may evaluate:

- Position.
- Orientation.
- Route segment and progress.
- Visual trail.
- Optional speed/heading annotations.

It must preserve the declared route coordinate semantics: Local Cartesian, Projected, Geodesic, and Bézier are not interchangeable. Route parameterization must be explicit.

Orientation defaults to the forward route tangent unless a movement descriptor explicitly declares another orientation rule. Reverse playback does not automatically reverse the Unit's facing direction.

### 6.4 Trails

The presentation distinguishes:

- Formal planned route.
- Historical traversed trail.
- Future route.
- User-configured visible trail window.

A visual trail is derived from authoritative route/time data and must not become a second persisted movement history.

### 6.5 Multiple Units

Multiple Units are evaluated independently and may be processed in parallel. Published pose collections must have stable ordering and a common Presentation Context.

---

## 7. Terrain as an upstream environment dependency

Terrain may influence:

- Movement planning and route feasibility.
- Slope and terrain movement cost.
- Speed/time estimation where the movement model requires it.
- Unit elevation and terrain-conforming visual placement.
- Terrain rendering and terrain-aware labels/symbol placement.

Terrain does not become an implicit renderer-owned movement authority.

Rules:

1. Historical terrain must be time-aware when historical movement depends on it.
2. Current terrain must never silently substitute for missing historical terrain.
3. NoData is not zero elevation and is not automatically traversable.
4. Playback evaluates the route/descriptor already established by authoritative planning; it does not silently re-plan or rewrite history.
5. A terrain change that should alter historical movement requires an explicit upstream domain/planning operation and corresponding authoritative state change.
6. Terrain dataset identity/version belongs in the Presentation Context when it affects output.

V1 may support basic elevation/slope queries, water/obstacle constraints, and simple configurable movement-cost estimates. Weather, mud, fatigue, detailed physics, and autonomous tactical rerouting remain outside the initial renderer baseline.

---

## 8. Text & Label Engine

### 8.1 Boundary

Text & Label Engine is independent of the Renderer. It receives authoritative text content and presentation metadata, then produces immutable label layout output.

It owns:

- Text shaping through the configured single shaping authority.
- Font fallback resolution and Typeface identity.
- Glyph metrics and text-run segmentation.
- Candidate anchor generation.
- Collision detection and placement ranking.
- Scale/zoom visibility bands and density budgets.
- Manual placement preferences.
- Immutable LabelLayoutResult publication.

It does not own historical text facts or mutate Object Geometry.

### 8.2 Single shaping authority

HarfBuzz remains the single shaping authority for the production text pipeline.

- Do not introduce a second independent shaping engine.
- Shaping metrics used for collision/layout must correspond to the glyph data ultimately drawn.
- Font fallback must retain the actual Typeface used for each shaped run.
- Glyph ID, Typeface, advance, offset, cluster, baseline, and direction data must remain associated through layout and drawing.
- Latin, CJK, mixed scripts, RTL, punctuation, and combining marks are required validation cases.

### 8.3 Production glyph drawing policy

**Prohibited:** constructing a `GlyphRun` manually and passing it to Avalonia `DrawingContext.DrawGlyphRun()`.

This prohibition is mandatory even though the Avalonia framework root cause has not been confirmed.

Allowed production routes:

**Route A — preferred map-canvas route: Skia glyph-ID drawing**
- Consume the shaped glyph IDs and associated Typeface/positioning data from the single shaping pipeline.
- Draw glyphs through Skia without asking Skia to perform a second shaping pass.
- Ensure the Typeface and glyph metrics used for measurement are the same as those used for drawing.
- Batch glyph output where practical; do not rebuild expensive text blobs every frame without a measured reason.

**Route B — Avalonia TextLayout drawing**
- Avalonia TextLayout may produce GlyphRun objects for Avalonia drawing APIs.
- It must use the configured HarfBuzz shaping path.
- Only GlyphRun objects produced by Avalonia TextLayout may enter Avalonia DrawingContext drawing calls.
- Do not convert a hand-built GlyphRun into an Avalonia draw call as a shortcut.

The two routes must not be mixed for the same text run in a way that creates different shaping metrics or fallback decisions.

### 8.4 Label placement

- Candidate generation is bounded and deterministic.
- Collision tests use viewport logical pixels.
- Priority, class, and stable tie-breaking are explicit.
- V1 may use deterministic greedy placement with a bounded candidate set.
- Hysteresis may reduce visual flicker but must not cause labels to depend on hidden mutable history.
- Manual pinned/preferred/automatic placement must declare its scope (Map, Branch, or other approved context).
- Unit labels consume the pose from the same Presentation Context as Animation Runtime.
- Long-form Narration is not automatically converted into map labels. An explicit MapAnnotation projection is required.

### 8.5 Cache identity

Label layout cache keys include at least:

- Presentation Context fingerprint.
- Text content/revision.
- Font/fallback configuration revision.
- Style revision.
- Viewport generation and scale band.
- Manual placement revision.
- Algorithm version.

---

## 9. Map Decorations

Map Decorations are independent from Map Objects and from ordinary label placement.

V1 decoration types:

- Legend.
- Scale Bar.
- North Arrow.
- Grid.
- Map Frame.
- Title.
- Attribution.

Decorations are layout elements in the map presentation/layout workspace. They do not become domain Objects merely because they are drawn on the map.

A scale bar must respect CRS capabilities. If meaningful ground distance cannot be derived, the UI must communicate the limitation rather than inventing a distance.

---

## 10. Scene model and assembly

### 10.1 Renderer-neutral scene

Scene Assembly converts compatible presentation outputs into a backend-neutral immutable Scene Snapshot.

The scene model may contain:

- Geometry draw items.
- Symbol/marker draw items.
- Shaped text/glyph draw items.
- Unit pose and trail draw items.
- Decoration draw items.
- Clip groups and layer ordering.
- Style/material references or resolved immutable style values.
- Stable source/provenance identifiers for diagnostics and interaction mapping.

The scene model must not contain mutable Domain entities, database handles, Avalonia Controls, ViewModels, or backend canvas objects.

### 10.2 Atomic publication

- A complete snapshot is built off the render callback.
- The publisher atomically swaps one immutable snapshot reference.
- A draw operation captures one reference at its start and uses it for the entire draw.
- A frame never fetches separate geometry, label, and pose snapshots during rendering.
- Snapshot versions are monotonically increasing.
- Cancellation or failure must never publish a partial snapshot.
- The old snapshot remains valid until no draw operation can reference it.

### 10.3 Scene provenance

Each draw item should retain stable source identity sufficient for diagnostics and, where required, hit-test mapping. Provenance is presentation metadata; it is not a new domain identity.

---

## 11. Avalonia and Skia host integration

### 11.1 Desktop UI boundary

Avalonia owns:

- Main window and application shell.
- Toolbars, panels, dialogs, menus, property editors.
- Input routing and normal control interaction.
- Map viewport hosting and lifecycle.

Skia owns the map viewport drawing backend only. Ordinary controls remain Avalonia controls.

### 11.2 Custom draw operation

The map viewport uses Avalonia's supported custom drawing extension point, validated against the exact Avalonia 12 API in TV-01.

Implementation requirements:

- Set accurate operation bounds.
- Acquire the Skia drawing lease inside the callback and release it within the documented lifetime.
- Do not retain SKCanvas, ImmediateDrawingContext, lease, or other callback-scoped objects.
- Draw only from the captured immutable Scene Snapshot.
- Keep callback work bounded and avoid allocations or resource rebuilds that can be prepared earlier.
- Avalonia scene-graph hit testing is not a substitute for Spatial Presentation hit testing.

### 11.3 Resource ownership

Long-lived immutable resources (for example, prepared path or glyph resources) must have explicit ownership and disposal rules. Callback-scoped resources must not escape the callback.

Resource caches must be keyed by all inputs that affect their contents, including Typeface/font revision, style revision, projection or geometry revision where relevant, and backend/device generation.

Device loss or backend recreation must invalidate backend-specific resources without invalidating authoritative domain or presentation data.

---

## 12. Threading and scheduling model

| Execution area | Allowed work | Prohibited work |
|---|---|---|
| Avalonia UI thread | Controls, input, viewport state, scheduling, snapshot reference acquisition | Long-running projection/index/label work |
| Background workers | Projection, spatial index build, animation evaluation, label layout, scene assembly, immutable resource preparation | Direct mutation of Avalonia controls or Domain state |
| Render callback | Read one immutable snapshot, acquire backend lease, issue drawing commands | Domain/database/filesystem/ViewModel access, blocking waits, UI dispatch, writes |
| Core Domain/application boundary | Historical reconstruction, validation, commands, persistence | Backend-specific drawing work |

Rules:

- Never synchronously block the render callback waiting for the UI thread or a worker.
- Background jobs receive immutable inputs.
- Cancellation is cooperative; cancelled or stale results are not published.
- Worker completion order must not affect final scene ordering or deterministic tie-breaks.
- Shutdown cancels outstanding work and releases resources safely.
- Avalonia text APIs and font services must not be assumed thread-safe without explicit validation. Prefer preparing shaped data on a controlled worker path with documented ownership, or serialize access where the selected API requires it.

---

## 13. Caching and invalidation

Caches are derived and disposable. They must never become an alternative source of truth.

| Cache | Minimum identity / invalidation |
|---|---|
| Projection cache | Geometry revision + source/target CRS + projection version |
| Spatial index | Geometry snapshot revision + coordinate-space definition |
| Animation pose | Context fingerprint + movement descriptor revision + terrain/profile revisions |
| Label layout | Context fingerprint + font/style/text/manual-placement revisions |
| Geometry tessellation | Geometry revision + projection + tolerance/scale + tessellator version |
| Glyph resources | Shaped run identity + Typeface/font revision + backend/device generation |
| Scene snapshot | Complete Presentation Context fingerprint + scene assembler version |

Invalidation must be explicit. Avoid broad global cache flushes when a precise revision key is available. Avoid premature persistent caches; V1 should prefer in-memory derived caches unless measurement justifies persistence.

---

## 14. Performance strategy

TV-01 established that naive per-primitive immediate drawing is not a production performance strategy. In the observed Windows test environment, approximately 50K visible primitives took about 92.2 ms per frame (roughly 11 FPS); the earlier 6.2 ms observation was invalid because incorrect camera parameters culled most of the scene.

This evidence is a warning, not a production benchmark or a product performance target.

Production strategy:

1. Batch compatible geometry by style/material where ordering semantics permit.
2. Use level of detail and scale-dependent simplification.
3. Use conservative viewport culling and spatial indexes.
4. Avoid rebuilding glyph resources and tessellation on every frame.
5. Separate static and dynamic scene portions where this reduces work without breaking snapshot consistency.
6. Keep immutable snapshot publication atomic even if preparation is incremental internally.
7. Measure scene build, label layout, resource preparation, and render time separately.
8. Record visible item count and culled item count so benchmark results cannot hide incorrect culling.

No performance claim is accepted without a documented fixture, camera/viewport parameters, visible item count, machine, backend, and capture method.

---

## 15. Failure behavior and diagnostics

The presentation system must fail visibly and safely:

- Invalid projection: report the feature/context and omit or mark the affected item; never silently place it at origin.
- Stale async result: discard and increment a diagnostic counter.
- Missing font: use declared fallback and record the resolved Typeface; do not silently substitute incompatible metrics.
- Missing historical terrain: report unavailable historical input; do not substitute current terrain.
- NoData terrain: preserve NoData semantics.
- Snapshot assembly failure: keep the last valid snapshot and report failure; never publish partial output.
- Backend/device reset: recreate backend resources from immutable presentation data.
- Invalid decoration scale: show unavailable/unsupported rather than a fabricated scale.

Diagnostics should include ContextFingerprint, snapshot version, Map/Branch/time identifiers, viewport generation, projection ID, style revision, visible/candidate item counts, stale-result count, and stage timings.

---

## 16. TV-01 toolbar text corruption — accepted constraint

### 16.1 Evidence status

The TV-01 follow-up establishes:

- In the prototype, manually constructed GlyphRun objects passed to Avalonia DrawGlyphRun reproduce toolbar text corruption.
- Avalonia TextLayout-produced GlyphRun objects passed to the same DrawGlyphRun path were clean in the repeated comparison.
- Constructing/shaping without drawing was clean.
- The independent minimal reproduction project did not reproduce the issue.
- Removing DrawGlyphRunSamples from the normal render path restored clean Debug and Release toolbar rendering.
- The reported test suite passed 33/33.

**Overall diagnosis remains Strongly Indicated.** The prototype-local trigger is confirmed; an Avalonia Framework defect is not confirmed. No upstream Avalonia issue should be filed without an independent reproducer.

### 16.2 Architecture decision

The issue is closed as an architecture blocker. The implementation constraint in §8.3 is mandatory:

> Do not pass manually constructed GlyphRun objects to Avalonia DrawingContext drawing APIs. Use the Skia glyph-ID route or Avalonia TextLayout-produced GlyphRun route.

This does not prohibit HarfBuzz shaping data or its use in Skia glyph-ID rendering. It prohibits the unsafe/unverified hand-built GlyphRun → Avalonia DrawingContext route.

### 16.3 Reopening criteria

Reopen the framework investigation only if:

- A production requirement cannot be met by the approved drawing routes; or
- A standalone minimal reproducer is obtained and the upstream issue can be described with exact versions, steps, and evidence.

macOS runtime remains unverified and must be tracked separately.

---

## 17. Validation and acceptance gates

### 17.1 Functional gates

- Correct map/branch/historical-time context propagation.
- Deterministic scene result for equal context inputs.
- Projection and coordinate conversion tests.
- Geometry bounds, tessellation, hit testing, and snap ranking tests.
- Bézier and mixed-geometry rendering tests.
- Unit pose tests for forward, reverse, pause, seek, and arbitrary-time evaluation.
- Terrain NoData and historical-data behavior tests.
- Label shaping tests for Latin, CJK, mixed, RTL, punctuation, and combining marks.
- Typeface/fallback/metrics consistency tests.
- Decoration and CRS-aware scale-bar tests.
- Atomic snapshot publication and stale-result discard tests.

### 17.2 Runtime and platform gates

- Windows Debug and Release build/run.
- macOS build and actual runtime validation (not just publish).
- DPI tests at available 100%, 125%, 150%, and 200% scales.
- Resize, pan, zoom, minimize/restore, and device/backend recreation.
- Repeated open/close and shutdown with outstanding background work.
- No resource lifetime violations or render callback blocking.

### 17.3 Performance gates

Use repeatable visible fixtures at 1K, 10K, and 50K items, with documented camera and viewport settings. Record:

- Scene assembly time.
- Projection/index preparation time.
- Animation evaluation time.
- Label layout time.
- Render time.
- Allocations and memory observations.
- Visible versus culled counts.
- Backend and device information.

Targets are to be set after a valid batched/LOD baseline is measured. Do not promote TV-01 naive immediate-mode numbers into production SLAs.

---

## 18. Implementation sequence

Recommended production implementation order:

1. **PRA-01 — Presentation Context and immutable Scene Snapshot**
   - Context identity, fingerprint, monotonic snapshot version, atomic publication, stale-result rejection.
2. **PRA-02 — Avalonia/Skia production viewport seam**
   - Custom draw operation, lease lifecycle, resource ownership, coordinate/DPI boundary.
3. **PRA-03 — Spatial Presentation foundation**
   - Typed coordinate spaces, projection interface, geometry conversion, bounds, index, hit testing.
4. **PRA-04 — Text & Label production path**
   - Single HarfBuzz authority, Skia glyph-ID route, fallback consistency, layout and collision.
5. **PRA-05 — Scene batching, LOD, and cache invalidation**
   - Measured rendering pipeline, visible-count diagnostics, resource reuse.
6. **PRA-06 — Animation Runtime integration**
   - Deterministic Unit pose and trail integration under shared Presentation Context.
7. **PRA-07 — Decorations and layout integration**
   - Legend, scale bar, north arrow, grid, frame, title, attribution.
8. **PRA-08 — Terrain integration and cross-platform validation**
   - Historical terrain inputs, NoData semantics, Windows/macOS runtime and performance verification.

This sequence permits the rendering foundation to progress without prematurely coupling all presentation subsystems.

---

## 19. Open items and explicit non-blockers

| Item | Status | Impact |
|---|---|---|
| Avalonia toolbar root cause beyond prototype-local trigger | Unresolved / Strongly Indicated | Not a V0.2 blocker; prohibited path avoids exposure |
| Independent Avalonia reproducer | Not available | No upstream issue |
| macOS runtime | Not yet verified | Must pass before cross-platform release |
| Final production performance targets | Pending valid batched/LOD measurements | Does not block architecture baseline |
| Exact projection catalogue and CRS coverage | Separate spatial/projection design and validation | Renderer consumes declared projection service |
| Canonical terrain dataset/storage format | Separate terrain architecture decision | Renderer remains format-agnostic |
| Advanced movement physics | Deferred | Basic terrain-aware movement integration only |

---

## 20. Relationship to existing repository documentation

This document defines the **Avalonia 12 + .NET 10 + Skia production presentation target** established by the current project direction and TV-01 validation.

The repository's older top-level README / AGENTS material still contains Rust + Bevy implementation-era descriptions. Those documents describe the historical/current repository implementation and must not be silently treated as the production target defined here. A separate documentation-alignment change must reconcile the top-level architecture map, dependency rules, glossary, and ADR index with the accepted Avalonia direction before implementation work is considered fully integrated.

Until that alignment is completed:

- This V0.2 document is the production presentation architecture reference.
- TV-01 reports remain evidence records, not production architecture.
- Old Bevy-specific module and crate boundaries are not to be copied into the Avalonia implementation.
- Core Domain authority, local-first project principles, non-destructive history, and time-driven state semantics remain applicable unless explicitly superseded by an accepted decision.

---

## 21. Change control

Changes to the following require an explicit architecture review and decision record:

- Core Domain versus Presentation authority boundary.
- Presentation Context identity or consistency semantics.
- Snapshot immutability / atomic publication model.
- Text shaping authority or the prohibited GlyphRun route.
- Renderer callback thread and resource-lifetime rules.
- Historical movement and terrain authority.
- CRS/coordinate-space semantics.

Ordinary implementation choices (data structures, batching thresholds, cache sizes, internal class names) may be decided by the implementation team when they preserve these contracts and are supported by tests or measurements.
