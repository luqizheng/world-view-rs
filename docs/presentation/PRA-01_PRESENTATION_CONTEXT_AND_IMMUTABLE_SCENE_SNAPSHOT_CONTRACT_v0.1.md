# PRA-01 — Presentation Context & Immutable Scene Snapshot Contract V0.1

- **Date:** 2026-11-21
- **Status:** Developer-ready technical contract
- **Parent architecture:** [Production Renderer Architecture V0.2](./PRODUCTION_RENDERER_ARCHITECTURE_v0.2.md)
- **Scope:** Presentation Context identity, asynchronous evaluation coordination, immutable scene snapshots, atomic publication, and stale-result rejection
- **Implementation target:** C# / .NET 10; Avalonia 12 host integration is limited to notification/invalidation boundaries
- **Authority:** This contract does not replace Core Domain or Domain Service contracts

---

## 0. Purpose and decision

PRA-01 establishes the shared consistency mechanism used by every production presentation subsystem.

For one requested map view, all derived presentation results must describe the same authoritative world state and the same presentation inputs. A frame must never combine geometry from one historical time with Unit poses, labels, or decorations produced for another context.

The system therefore uses:

1. An immutable **Presentation Context** describing all inputs that affect one presentation evaluation.
2. A deterministic **Context Fingerprint** identifying that context.
3. A separate monotonic **Request Generation** controlling asynchronous work ordering.
4. One immutable, backend-neutral **Scene Snapshot** containing a complete assembled result.
5. One **Scene Snapshot Publisher** that atomically publishes complete snapshots and rejects stale work.

The renderer reads one snapshot reference per draw. It does not assemble, fetch, or reconcile subsystem results.

### 0.1 Normative terms

- **MUST / MUST NOT:** mandatory contract.
- **SHOULD / SHOULD NOT:** recommended unless a documented reason exists.
- **MAY:** optional implementation choice that preserves the contract.

---

## 1. Responsibility and authority

### 1.1 Presentation Context Builder

The builder receives already-resolved authoritative identities and presentation configuration from application services. It creates an immutable context value and computes its fingerprint.

It MUST NOT:
- Query persistence directly.
- Invent or normalize domain historical semantics.
- Change a Map, Branch, World State, projection, or style.
- Read mutable Avalonia controls from background workers.

### 1.2 Presentation Request Coordinator

The coordinator owns the latest requested context and a monotonic request generation for one viewport/presentation session. It starts/cancels background preparation and decides whether a completed build is still eligible for publication.

It does not own map truth or rendering resources.

### 1.3 Scene Assembler

The assembler accepts only immutable, context-tagged outputs from upstream presentation services. It verifies compatibility and creates one complete immutable Scene Snapshot.

It MUST reject:
- A required subsystem result with a different ContextFingerprint.
- A missing required subsystem result.
- A duplicate/conflicting result for the same required output slot.
- A result built from a stale or invalidated request.

Optional subsystems may be absent only when the context explicitly marks them as not applicable or disabled. Absence must not be silently interpreted as a successful empty result.

### 1.4 Scene Snapshot Publisher

The publisher is the only component allowed to replace the currently published snapshot reference for a viewport session.

It MUST publish complete snapshots atomically and MUST NOT mutate a snapshot after publication.

### 1.5 Renderer

The renderer captures exactly one immutable snapshot reference at the start of a draw operation and uses only that reference for the entire operation.

It MUST NOT call the coordinator, assembler, Domain Services, database, or ViewModels.

---

## 2. Presentation Context contract

### 2.1 Required fields

A context contains immutable, canonical values for:

| Field | Required | Description |
|---|---:|---|
| MapId | Yes | Stable identity of the map being presented |
| BranchId | Yes | Explicit branch identity, including the project's canonical root-branch identity |
| HistoricalTime | Yes | Canonical domain time value, not formatted display text |
| WorldStateRevision | Yes | Stable revision/fingerprint of the exact authoritative state supplied to presentation |
| ProjectionDefinitionId | Yes | Identity of coordinate interpretation/projection |
| ProjectionDefinitionVersion | Yes | Version of the projection definition/implementation contract |
| StyleRevision | Yes | Effective immutable style/symbol configuration revision |
| ViewportGeneration | Yes | Monotonic generation for viewport-dependent inputs |
| ViewportDescriptor | Yes | Logical size, camera transform, clip/visible region, and logical-to-device scale |
| FontConfigurationRevision | Yes | Font inventory/fallback configuration identity |
| TerrainDescriptor | Conditional | Required when any requested output depends on terrain; identifies dataset and historical revision/time semantics |
| ContextFingerprint | Derived | Stable fingerprint of all fields that affect this evaluation |

The exact .NET types must use existing project/domain ID and Historical Time types where they exist. Do not create competing ID/time types merely for this contract.

### 2.2 Canonical historical time

HistoricalTime MUST be supplied by the authoritative Timeline/Core Domain boundary.

- Do not use localized date strings, formatted labels, floating-point frame counters, or wall-clock timestamps as substitutes.
- Preserve the domain's exact ordering and precision semantics.
- Do not round time when building a context.
- Two times that are semantically distinct in Core Domain MUST produce distinct context fingerprints.

### 2.3 World State revision

WorldStateRevision identifies the exact World State data used to build presentation outputs.

- It may be a domain revision ID or a stable content fingerprint supplied by the authoritative layer.
- Presentation MUST NOT manufacture a revision by hashing only the visible subset of objects.
- If no stable revision is available, the upstream integration must supply an explicit immutable snapshot identity. A timestamp alone is not an acceptable substitute.

### 2.4 Viewport descriptor

ViewportDescriptor contains only values that affect viewport-dependent output:

- Logical viewport width and height.
- Camera/map-to-viewport transform.
- Visible/clipping region or an equivalent canonical representation.
- Logical-to-device scale (DPI/render scaling).
- Any additional viewport setting that changes culling, hit geometry, label placement, or scene output.

Values MUST be normalized before fingerprinting:
- Reject NaN and infinity.
- Normalize negative zero to zero.
- Use a documented deterministic numeric representation.
- Do not fingerprint object hash codes, runtime memory addresses, or culture-sensitive strings.

A viewport generation changes whenever any descriptor value affecting output changes. Generation is monotonic within a viewport session and is not reused after resize/transform changes.

### 2.5 Conditional terrain identity

If terrain affects any requested presentation result, TerrainDescriptor MUST identify the exact terrain input, including dataset identity/version and historical applicability.

If terrain is irrelevant to the requested scene, it may be absent. Missing terrain data must never be represented as a fabricated revision or as zero elevation.

### 2.6 Context immutability

After construction:
- All context properties are read-only.
- Referenced collections are immutable or defensively copied.
- No mutable service/configuration object may be retained.
- A changed input creates a new context value and fingerprint.

---

## 3. Context fingerprint

### 3.1 Purpose

ContextFingerprint is the stable identity for derived presentation results. It is used for compatibility checks, cache identity, diagnostics, and stale-result rejection.

### 3.2 Requirements

The fingerprint algorithm MUST:
- Be deterministic across process runs for equal canonical inputs.
- Include every presentation-relevant context field.
- Exclude runtime-only values such as object references, process IDs, thread IDs, wall-clock creation time, and request generation.
- Use an explicit algorithm/version identifier.
- Avoid relying on .NET GetHashCode().
- Be encoded in a stable, documented form (for example, a versioned SHA-256 digest).

The exact algorithm can be selected by the developer, provided the above properties are tested and documented.

### 3.3 Fingerprint versioning

The serialized fingerprint input has a schema/version identifier. Any change to canonicalization or included fields increments that version.

Old cache entries with a different fingerprint schema are misses, not candidates for reuse.

### 3.4 Cache keys and context subsets

ContextFingerprint is the safe default key for whole-scene outputs.

A subsystem MAY use a narrower cache key only when it proves that omitted context fields cannot affect that cached artifact. The cache key must still include every relevant source revision, algorithm version, and configuration input. Do not use a single broad context key where a precise key is necessary for reuse, and do not weaken correctness for cache hit rate.

---

## 4. Request generation and asynchronous ordering

ContextFingerprint identifies semantic inputs. It does not by itself order requests.

### 4.1 Request Generation

Each new presentation request receives a monotonically increasing RequestGeneration within its viewport session.

RequestGeneration:
- Is assigned by the coordinator.
- Is not part of ContextFingerprint.
- Is not persisted as domain data.
- Is used to distinguish repeated requests for the same context and to prevent older work from publishing after newer work.

### 4.2 Superseding requests

When a new request supersedes an earlier request:
1. Increment RequestGeneration.
2. Record the new requested context/fingerprint.
3. Cancel the prior request cooperatively.
4. Start preparation using immutable inputs.
5. Treat any prior completion as stale, even if it finishes successfully.

Cancellation is an optimization, not the correctness mechanism. Generation and fingerprint validation are mandatory because cancellation may arrive too late.

### 4.3 Publish eligibility

A completed build is publishable only if all are true:
- Its RequestGeneration equals the coordinator's current generation.
- Its ContextFingerprint equals the currently requested context fingerprint.
- It passed all required assembly validations.
- It was not cancelled or marked failed.
- Its snapshot version can be assigned by the publisher.

This prevents out-of-order worker completion from reverting the viewport to an older requested state.

### 4.4 Same-context repeated request

Two requests may have the same ContextFingerprint but different RequestGeneration (for example, an explicit refresh). Only the latest generation may publish. Identical context does not make an older request eligible.

---

## 5. Presentation result envelope

Every asynchronous subsystem result MUST carry:

- ContextFingerprint.
- RequestGeneration (or an immutable request token that resolves to it).
- Result type/slot identity.
- Immutable result payload.
- Completion status.
- Optional diagnostics: stage duration, warnings, and source revision metadata.

A result must not be accepted based only on MapId or HistoricalTime. Full context compatibility is required.

Required/optional slots are declared by the request plan before assembly begins. The assembler must not infer that a missing result is empty.

Errors are explicit. A failed required stage fails the current assembly; a failed optional stage may be omitted only under an explicit policy recorded in the request plan and diagnostics.

---

## 6. Immutable Scene Snapshot

### 6.1 Required metadata

A published Scene Snapshot contains:

| Field | Meaning |
|---|---|
| SnapshotVersion | Monotonically increasing publisher-assigned version |
| Context | Immutable Presentation Context |
| ContextFingerprint | Fingerprint copied from Context |
| RequestGeneration | Generation that produced this snapshot |
| CreatedAtUtc | Diagnostic timestamp only; never used for semantic ordering |
| ScenePayload | Complete immutable renderer-neutral scene data |
| Diagnostics | Immutable stage timings/counts/warnings suitable for inspection |

SnapshotVersion is assigned only at publication and increases monotonically within the publisher's lifetime. It is not a domain revision.

### 6.2 Immutability

After publication:
- No property may change.
- No collection may be mutated.
- No referenced payload may expose mutable internal storage.
- Arrays must be privately owned and never returned as mutable references; use immutable collections or read-only wrappers with defensive ownership.
- Lazy evaluation that mutates observable state is prohibited.
- The snapshot must not contain Avalonia Controls, ViewModels, database handles, mutable Domain entities, SKCanvas, drawing leases, or callback-scoped resources.

### 6.3 Backend-neutral scene data

ScenePayload contains renderer-neutral values and stable identifiers. Backend-specific objects (for example, SKPath, SKTextBlob, GPU textures, or device-bound resources) belong in a separately managed resource cache keyed by immutable source identity and backend/device generation.

This avoids coupling snapshot lifetime to graphics device lifetime.

### 6.4 Empty scene versus incomplete scene

An intentionally empty map is a valid complete snapshot with an explicit empty ScenePayload.

A scene with missing required results is incomplete and MUST NOT be published.

The two states must be distinguishable in diagnostics and tests.

---

## 7. Atomic publication and read semantics

### 7.1 Publication

The publisher performs one atomic reference replacement of the complete snapshot.

- Build and validate off the render callback.
- Assign SnapshotVersion centrally.
- Atomically replace the published reference.
- Notify the UI/render host that a new frame is available without synchronously waiting for rendering.
- Never publish intermediate geometry-only, labels-only, or Unit-pose-only snapshots as if they were complete.

### 7.2 Draw capture

At the beginning of a draw:
1. Read the current snapshot reference once using the appropriate atomic reference operation.
2. Store it in a local variable.
3. Use that local reference for every draw stage.
4. Do not reread the publisher during that draw.

The snapshot remains valid because it is immutable and managed by normal object lifetime. If backend resources are required, their own leases/lifetime rules are handled separately.

### 7.3 Old snapshot while a new context is building

The UI may retain the last complete snapshot while a new request is being prepared, but it MUST NOT present it as if it represents the newly requested context.

The host must expose a stale/loading state (for example, a subtle progress indicator or status metadata). Product UX may choose to keep the old frame visible to avoid flicker, but the displayed frame's ContextFingerprint remains the old one and diagnostics must make that explicit.

If product semantics require that a historical-time change never briefly shows the prior time as current, the host may blank the viewport until the new snapshot is ready. This is a UI policy; the publisher contract remains unchanged.

### 7.4 Build failure

If the current request fails:
- Do not publish a partial snapshot.
- Keep the last complete snapshot available as stale content or clear it according to host policy.
- Expose failure status and diagnostics for the current request.
- Do not silently relabel the old snapshot with the new context.

---

## 8. Concurrency and lifecycle

### 8.1 Ownership

- Context and Scene Snapshot are immutable values.
- Coordinator state (current generation, requested fingerprint, cancellation source) is synchronized and has a single logical owner.
- Publisher reference replacement is atomic.
- Scene assembly may run on background workers.
- Renderer only reads a captured snapshot.
- UI thread owns UI controls and invalidation requests.

### 8.2 Shutdown

On viewport/session shutdown:
1. Mark the coordinator closed.
2. Increment/invalidate the request generation.
3. Cancel active work.
4. Reject all later completions.
5. Stop notifications to disposed UI targets.
6. Release coordinator-owned cancellation resources.
7. Allow immutable snapshots to be garbage-collected when no readers retain them.

No worker may synchronously wait for the UI thread during shutdown.

### 8.3 No premature disposal

Scene Snapshot is immutable managed data. Do not implement manual disposal on it unless it owns a resource whose lifetime cannot be separated; the preferred design is to keep backend resources outside the snapshot.

If a future resource must be snapshot-scoped, introduce an explicit reference-counted lease contract in a separate reviewed change. Do not add speculative ref-counting now.

---

## 9. Diagnostics and observability

Every request/build/publication diagnostic should include:

- ViewportSessionId (diagnostic/session identity only).
- RequestGeneration.
- ContextFingerprint and fingerprint schema version.
- MapId, BranchId, HistoricalTime (safe canonical diagnostic representation).
- WorldStateRevision.
- ViewportGeneration and descriptor summary.
- SnapshotVersion when published.
- Stage names and durations.
- Required/optional result slots and their completion state.
- Stale/cancelled/rejected reason.
- Visible item counts where available.

Do not log full user content or sensitive data merely to identify a context. Stable IDs and revision identities are normally sufficient.

Minimum counters:
- requests started;
- requests superseded/cancelled;
- builds succeeded/failed;
- stale results rejected;
- snapshots published;
- incomplete assemblies rejected;
- render invalidations requested.

---

## 10. Suggested C# contract shape

The following is illustrative, not a requirement to copy class names or introduce a separate assembly. Reuse existing project types.

```csharp
public sealed record PresentationContext(
    MapId MapId,
    BranchId BranchId,
    HistoricalTime HistoricalTime,
    WorldStateRevision WorldStateRevision,
    ProjectionDefinitionId ProjectionDefinitionId,
    string ProjectionDefinitionVersion,
    StyleRevision StyleRevision,
    long ViewportGeneration,
    ViewportDescriptor Viewport,
    FontConfigurationRevision FontConfigurationRevision,
    TerrainDescriptor? Terrain,
    ContextFingerprint Fingerprint);

public sealed record PresentationRequestToken(
    long RequestGeneration,
    ContextFingerprint ContextFingerprint);

public sealed record SceneSnapshot(
    long SnapshotVersion,
    PresentationContext Context,
    long RequestGeneration,
    DateTimeOffset CreatedAtUtc,
    ScenePayload Payload,
    SceneDiagnostics Diagnostics);

public interface ISceneSnapshotPublisher
{
    SceneSnapshot? Current { get; }

    bool TryPublish(
        PresentationRequestToken request,
        SceneSnapshotCandidate candidate);
}
```

Implementation notes:
- The builder, not callers, computes Fingerprint; callers must not inject an arbitrary fingerprint.
- In a real implementation, avoid accepting a candidate that can claim a different context/token than the publisher's validated request.
- Prefer separate candidate and published snapshot types so SnapshotVersion is assigned only by the publisher.
- `Current` must be a thread-safe atomic read.
- The sample omits error/status APIs for brevity; the implementation must expose them as specified above.
- Do not force a project-wide ID migration to match this sample.

---

## 11. Acceptance tests

The implementation is accepted only when automated tests demonstrate all of the following.

### 11.1 Context identity

1. Equal canonical inputs produce equal fingerprints across repeated constructions.
2. Changing each required context input changes the fingerprint.
3. Culture, process run, object allocation order, and runtime hash codes do not affect the fingerprint.
4. NaN/infinity are rejected; negative zero normalization is deterministic.
5. Distinct canonical Historical Time values remain distinct.
6. Terrain identity affects the fingerprint when terrain is relevant and is omitted only when explicitly irrelevant.

### 11.2 Request ordering

7. A newer request supersedes an older request.
8. An older request completing after the newer request cannot publish.
9. Cancellation arriving late cannot bypass generation validation.
10. Two requests with the same fingerprint but different generations still obey latest-generation publication.
11. A completion from a closed viewport session cannot publish or notify the UI.

### 11.3 Assembly and snapshot

12. Mixed ContextFingerprints are rejected.
13. Missing required output slots are rejected.
14. Explicitly empty valid scenes can publish.
15. Published snapshot payload cannot be mutated through exposed references.
16. SnapshotVersion is monotonically increasing.
17. Failed/cancelled builds never publish partial snapshots.
18. Snapshot contains no backend callback-scoped resources or mutable UI/domain objects.

### 11.4 Atomic reader behavior

19. Concurrent publication and drawing/read loops observe either the old complete snapshot or the new complete snapshot, never a partial mixture.
20. A simulated draw captures once and continues using the same snapshot even if a new one publishes during the draw.
21. The old snapshot remains readable while a newer one is being assembled.
22. Build failure leaves the previous snapshot unchanged and exposes current-request failure status.

### 11.5 Lifecycle and diagnostics

23. Shutdown cancels/invalidate outstanding requests and rejects late completion.
24. Stale rejection reason and request/snapshot identifiers are observable.
25. No render callback blocks waiting for a worker or UI dispatch.

Tests should be deterministic and headless where possible. Keep a small number of integration tests for the actual Avalonia viewport invalidation boundary.

---

## 12. Out of scope

PRA-01 does not implement:
- Projection algorithms or CRS catalogues.
- Spatial indexes, hit testing, snapping, or geometry editing.
- Animation evaluation.
- Text shaping or label placement.
- Terrain loading or raster rendering.
- Skia resource caches or batching.
- UI loading indicator design.
- Persistence of presentation snapshots.

These services integrate by tagging their immutable results with the context/request identity and obeying the assembly contract.

---

## 13. Implementation guidance and review gates

Prefer the smallest design that satisfies the contract:

- One coordinator per active map viewport/session.
- One immutable context value per request.
- One publisher per viewport/session.
- One complete scene snapshot per successful request.
- Existing .NET immutable collections/records where suitable.
- No message broker, distributed cache, database table, or global event bus.
- No speculative reference counting or multi-renderer abstraction.

Before implementation is considered complete, provide:
1. A short class/component map showing ownership.
2. The canonical fingerprint schema and tests.
3. The request-generation/cancellation state transition.
4. The publication/read atomicity implementation.
5. Test output for the acceptance list.
6. A small Avalonia integration proving snapshot invalidation without blocking the render callback.

Any proposed deviation that changes domain authority, historical-time semantics, atomicity, or stale-result rules requires architecture review. Ordinary naming and data-structure choices do not.
