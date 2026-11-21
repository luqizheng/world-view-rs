# PRA-01 Developer Handoff — Presentation Context & Immutable Scene Snapshot

- **Version:** V0.1
- **Date:** 2026-11-21
- **Status:** Ready for implementation
- **Parent:** `docs/presentation/PRODUCTION_RENDERER_ARCHITECTURE_v0.2.md`
- **Contract:** `docs/presentation/PRA-01_PRESENTATION_CONTEXT_AND_IMMUTABLE_SCENE_SNAPSHOT_CONTRACT_v0.1.md`
- **Target:** C# / .NET 10; Avalonia 12 host integration
- **Work type:** Production presentation foundation; no Core Domain feature implementation

---

## 1. Objective

Implement PRA-01 as the first production presentation foundation:

- Immutable Presentation Context and deterministic ContextFingerprint.
- Per-viewport request coordination with monotonic RequestGeneration.
- Context-tagged immutable subsystem result envelopes.
- Validation and assembly of one complete Scene Snapshot candidate.
- Atomic publication of complete immutable snapshots.
- Rejection of stale, cancelled, mixed-context, incomplete, and late-after-shutdown results.
- A minimal Avalonia integration that requests redraw when a new snapshot publishes, without blocking the render callback.

Read the full technical contract before coding. The contract is authoritative if this handoff omits a detail.

## 2. Scope

### In scope

1. Inspect the existing solution and identify current presentation-related code and reusable ID/time/revision types.
2. Implement the smallest maintainable PRA-01 component set.
3. Add deterministic unit and concurrency tests.
4. Add a minimal integration at the existing map viewport boundary (or a small focused harness if the production viewport does not yet exist).
5. Add diagnostics for request, rejection, build, and publication lifecycle.
6. Document ownership, fingerprint schema, request state transitions, and test commands.

### Out of scope

- Core Domain or database schema changes.
- Historical state reconstruction implementation.
- Projection/CRS algorithms.
- Spatial indexes, geometry editing, snapping, and hit testing.
- Animation Runtime.
- Text shaping or label placement.
- Terrain loading/rendering.
- Skia batching, LOD, GPU resource caches.
- A general-purpose event bus, message broker, distributed cache, or plugin framework.
- UI redesign beyond the minimum loading/stale/error status needed to avoid presenting an old frame as the new context.

## 3. Mandatory architecture rules

1. Core Domain / Domain Services remain the only authority for Map, Branch, Historical Time, and WorldStateRevision.
2. PresentationContext is immutable. Reuse existing project ID and time types; do not create parallel domain identities.
3. ContextFingerprint is computed from canonical values. Never use `GetHashCode()`, object identity, culture-formatted values, or timestamps as the fingerprint.
4. RequestGeneration is separate from ContextFingerprint. It is a monotonically increasing per-session ordering token and is not persisted as domain data.
5. Cancellation is best-effort only. Generation/fingerprint validation remains the correctness guard.
6. Only a complete, validated SceneSnapshotCandidate may be published.
7. One publisher assigns monotonically increasing SnapshotVersion and atomically swaps one immutable reference.
8. A render operation captures one snapshot reference once and uses it for the entire draw.
9. A failed or superseded build cannot publish a partial or stale result.
10. Keep backend-bound resources (SKCanvas, SKPath/device resources, drawing leases) out of SceneSnapshot. Snapshot data is backend-neutral.
11. Do not manually dispose immutable managed snapshots or add speculative reference counting.
12. No render callback may query Domain Services, persistence, ViewModels, mutable controls, or synchronously wait for workers/UI.

## 4. Suggested component responsibilities

Names may follow the existing codebase; preserve these responsibilities.

| Component | Responsibility |
|---|---|
| PresentationContextFactory | Build canonical immutable context and compute fingerprint |
| ContextFingerprintSerializer | Versioned deterministic canonical serialization |
| PresentationRequestCoordinator | Own latest request generation, cancellation, requested context |
| PresentationResultEnvelope<T> | Tag immutable stage output with request/context identity |
| SceneSnapshotAssembler | Validate required slots and context compatibility; create candidate |
| SceneSnapshotPublisher | Validate current request, assign version, atomic reference swap |
| PresentationDiagnostics | Record lifecycle counters and rejection reasons |
| ViewportInvalidationAdapter | Notify Avalonia host that a complete new snapshot is available |

Do not split each component into a separate project. A small set of cohesive classes/namespaces is preferred.

## 5. Required request lifecycle

Implement and test this logical flow:

```text
New viewport request
   |
   v
Create immutable PresentationContext
   |
   v
Coordinator increments RequestGeneration
   |
   +--> cancel previous request (best effort)
   |
   v
Prepare required presentation results in background
   |
   v
Assembler validates all result envelopes
   |
   +--> mixed context / missing required result / failure
   |        -> reject; expose diagnostics; publish nothing
   |
   v
Create complete SceneSnapshotCandidate
   |
   v
Publisher validates generation + fingerprint + session state
   |
   +--> stale / cancelled / closed -> reject
   |
   v
Assign SnapshotVersion and atomic reference swap
   |
   v
Request UI/render invalidation (non-blocking)
   |
   v
Render captures one snapshot reference and draws it
```

An old complete snapshot may remain visible while the new one is prepared, but the host must identify it as stale/loading. Never relabel an old snapshot with a new context.

## 6. Required implementation details

### 6.1 Fingerprint

- Define a schema/version constant.
- Use stable field ordering and explicit type tags/length boundaries.
- Use invariant canonical numeric/string representation.
- Reject NaN and infinity; normalize negative zero.
- Include all fields listed as required in the contract.
- Include terrain identity only when terrain affects the requested presentation.
- Add tests proving every field affects the fingerprint and equal inputs remain equal across repeated runs.

You may choose a suitable stable digest such as SHA-256. Do not build a generic serialization framework.

### 6.2 Request coordinator

- One coordinator per active viewport/session.
- Generation increments for every new request, including repeated requests with identical fingerprints.
- Superseding requests cancel the previous token.
- A completion must match both current generation and current fingerprint.
- Closed sessions reject all late completions and suppress UI notifications.
- Avoid holding locks while running expensive preparation or invoking callbacks.

### 6.3 Result assembly

- Required and optional result slots are declared by the request plan.
- Each result includes request identity and ContextFingerprint.
- Mixed-context results are rejected.
- Missing required slots are rejected.
- Empty scene is valid only when all required stages completed successfully and intentionally produced no draw items.
- Optional stage omission must be explicit and diagnostic.

### 6.4 Snapshot and publisher

- Separate candidate from published snapshot; publisher alone assigns SnapshotVersion.
- Snapshot contains immutable renderer-neutral data only.
- Atomic read/write must use correct .NET primitives and be reviewed for memory visibility.
- Publication must be serialized or compare-and-swap guarded so two concurrent completions cannot publish out of order.
- SnapshotVersion is monotonic within the publisher lifetime.
- A reader that captured the old snapshot may finish using it after a new snapshot publishes.

### 6.5 Avalonia boundary

- The publisher must not depend directly on controls.
- Use a small adapter/callback owned by the UI integration to request redraw after publication.
- Ensure notification is dispatched safely to the UI/render host according to Avalonia 12 APIs.
- Do not synchronously wait for UI dispatch.
- Keep the actual draw callback limited to atomic snapshot read + backend draw.

## 7. Tests and acceptance

Implement all applicable tests from §11 of the PRA-01 contract. At minimum, the report must include:

- Stable fingerprint and per-field invalidation.
- Culture-independent canonicalization.
- Out-of-order completion rejection.
- Same-fingerprint/different-generation rejection.
- Late cancellation cannot publish.
- Mixed context and missing required slot rejection.
- Explicit empty scene publication.
- Atomic snapshot replacement under concurrent readers.
- Single-capture draw behavior.
- Failed build preserves previous snapshot and exposes failure.
- Shutdown rejects late completions and notifications.
- Snapshot payload immutability and absence of UI/domain/backend objects.

Prefer headless deterministic tests. Keep Avalonia integration tests focused on the invalidation seam and exact Avalonia 12 API behavior.

## 8. Repository and change-scope safety

- Work in the existing repository and follow its current branch/worktree state.
- Before editing, inspect `git status`, current branch, and latest commit.
- Do not overwrite, reset, stash, or reformat unrelated user changes.
- In particular, preserve any unrelated user modifications in `GLOSSARY.md` and `docs/map-replacement/`.
- Do not assume a commit mentioned in a report is pushed. Report local HEAD and remote branch status separately.
- Do not change the old Rust/Bevy implementation or migrate Core Domain as part of PRA-01.
- Keep the change focused; list every modified/added path.

## 9. Deliverables

1. PRA-01 implementation code.
2. Automated tests and test output.
3. A concise implementation report at `docs/presentation/PRA-01_IMPLEMENTATION_REPORT.md`.
4. Updated README or presentation index only if a relevant index already exists and can be updated without broad unrelated restructuring.
5. Commit(s) on the agreed working branch, with exact SHA and push status.
6. Known limitations and deviations, each mapped to the contract section.

## 10. Completion report format

Report:

- Current branch and starting commit.
- Files added/changed.
- Component map and ownership.
- Fingerprint schema and exact included fields.
- Request-generation and cancellation behavior.
- Atomic publication/read implementation.
- Test commands and exact pass/fail counts.
- Avalonia integration behavior.
- Build warnings/errors.
- Deviations from contract (or “none”).
- Remaining risks.
- Commit SHA and whether it is present on remote.

Escalate only if implementation reveals a conflict with domain authority, canonical historical-time semantics, context identity, or atomic publication. Ordinary class names, collection types, and internal algorithms are developer decisions.

---

## 11. Definition of done

PRA-01 is done when:
- The technical contract's applicable acceptance tests pass.
- The implementation compiles in the supported target configuration.
- Stale/mixed/incomplete results cannot be published.
- A draw cannot observe a mixed snapshot.
- The Avalonia host is notified without blocking the render callback.
- No Core Domain or persistence authority has moved into Presentation.
- Implementation report and commit/push status are verifiable.
