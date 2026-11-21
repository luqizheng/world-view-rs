using System;
using System.Collections.Immutable;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Scene;

/// <summary>
/// Compact test context per Handoff §5.1. The production presentation context additionally
/// carries Map ID, Branch ID, canonical Historical Time, WorldState revision/fingerprint,
/// projection definition/version, style revision, viewport generation and a context
/// fingerprint. TV-01 fills representative values so the field shape is exercised without
/// touching Core Domain data.
/// </summary>
public sealed record SceneContext(
    Guid MapId,
    Guid BranchId,
    string HistoricalTimeLabel,
    long WorldStateRevision,
    string ProjectionId,
    int StyleRevision)
{
    public static SceneContext TestDefault(int revision = 1) => new(
        MapId: Guid.Parse("AAAAAAAA-0000-0000-0000-000000000001"),
        BranchId: Guid.Parse("BBBBBBBB-0000-0000-0000-000000000001"),
        HistoricalTimeLabel: "TV-01 synthetic present",
        WorldStateRevision: revision,
        ProjectionId: "tv01-planar-test-space",
        StyleRevision: 1);
}

/// <summary>Diagnostics carried inside the snapshot itself (Handoff §5.1).</summary>
public sealed record SceneBuildDiagnostics(
    DateTimeOffset CreatedAt,
    TimeSpan BuildDuration,
    int PrimitiveCount,
    int FeatureCount,
    int ThreadId);

/// <summary>One ordered render layer (minimal version of the contract's RenderLayerSnapshot).</summary>
public sealed record RenderLayerSnapshot(string LayerId, int ZOrder, ImmutableArray<ScenePrimitive> Primitives);

/// <summary>
/// Immutable scene snapshot - the renderer's ONLY scene input (Renderer Contract &amp; Scene Model).
///
/// Lifecycle: Building (thread-confined) -> Published (atomic swap in ScenePublisher) -> Retained
/// (until in-flight draws finish) -> Released. After publication this object is never mutated:
/// all collections are ImmutableArray, primitives are immutable records, and the class is sealed.
/// A draw operation captures exactly one reference and reads only it for the entire draw.
/// </summary>
public sealed class SceneSnapshot
{
    public SceneSnapshot(
        long version,
        SceneContext context,
        ImmutableArray<RenderLayerSnapshot> layers,
        SceneBuildDiagnostics buildDiagnostics)
    {
        if (version < 1) throw new ArgumentOutOfRangeException(nameof(version), "SnapshotVersion must be positive and monotonic.");
        if (layers.IsDefault) throw new ArgumentException("Layers must be initialized.", nameof(layers));

        Version = version;
        Context = context;
        Layers = layers;
        BuildDiagnostics = buildDiagnostics;
    }

    /// <summary>Monotonically increasing snapshot version. Never changes after construction.</summary>
    public long Version { get; }

    public SceneContext Context { get; }

    public ImmutableArray<RenderLayerSnapshot> Layers { get; }

    public SceneBuildDiagnostics BuildDiagnostics { get; }

    public int TotalPrimitiveCount
    {
        get
        {
            var n = 0;
            foreach (var layer in Layers)
                n += layer.Primitives.Length;
            return n;
        }
    }

    public override string ToString() =>
        $"SceneSnapshot v{Version} [{TotalPrimitiveCount} prims, {BuildDiagnostics.BuildDuration.TotalMilliseconds:F1} ms build]";
}
