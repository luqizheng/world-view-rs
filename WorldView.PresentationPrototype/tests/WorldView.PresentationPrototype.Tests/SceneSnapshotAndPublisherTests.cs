using System.Collections.Immutable;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Viewport;
using Xunit;

namespace WorldView.PresentationPrototype.Tests;

/// <summary>
/// Snapshot immutability and atomic publication tests (TV-05 / acceptance criteria 6, 7).
/// These tests need no Avalonia runtime (no fonts involved).
/// </summary>
public class SceneSnapshotAndPublisherTests
{
    private static SceneSnapshot MakeSnapshot(long version, int pointCount)
    {
        var builder = ImmutableArray.CreateBuilder<ScenePrimitive>(pointCount);
        for (var i = 0; i < pointCount; i++)
            builder.Add(new PointPrimitive(new MapPoint(i, i), 1.0, new ColorRgba(255, 0, 0)));

        var layers = ImmutableArray.Create(
            new RenderLayerSnapshot("only", 0, builder.ToImmutable()));

        return new SceneSnapshot(
            version,
            SceneContext.TestDefault((int)version),
            layers,
            new SceneBuildDiagnostics(System.DateTimeOffset.UtcNow, System.TimeSpan.Zero, pointCount, pointCount, 1));
    }

    [Fact]
    public void Snapshot_Version_Must_Be_Positive()
    {
        Assert.Throws<ArgumentOutOfRangeException>(() => MakeSnapshot(0, 1));
        Assert.Throws<ArgumentOutOfRangeException>(() => MakeSnapshot(-1, 1));
    }

    [Fact]
    public void TotalPrimitiveCount_Sums_Layers()
    {
        var snapshot = MakeSnapshot(1, 42);
        Assert.Equal(42, snapshot.TotalPrimitiveCount);
    }

    [Fact]
    public void Snapshot_Is_Insulated_From_Mutable_Builders_After_Construction()
    {
        // If builders leak into the snapshot, clearing the builder would shrink the snapshot.
        var builder = ImmutableArray.CreateBuilder<ScenePrimitive>(8);
        for (var i = 0; i < 8; i++)
            builder.Add(new PointPrimitive(new MapPoint(i, 0), 1, new ColorRgba(0, 0, 0)));
        var layer = new RenderLayerSnapshot("l", 0, builder.ToImmutable());
        var snapshot = new SceneSnapshot(1, SceneContext.TestDefault(), ImmutableArray.Create(layer),
            new SceneBuildDiagnostics(System.DateTimeOffset.UtcNow, System.TimeSpan.Zero, 8, 8, 1));

        builder.Clear();
        Assert.Equal(8, snapshot.TotalPrimitiveCount);
    }

    [Fact]
    public void First_Publish_Succeeds_And_Becomes_Current()
    {
        var publisher = new ScenePublisher();
        Assert.Null(publisher.Current);

        var snapshot = MakeSnapshot(1, 10);
        Assert.True(publisher.TryPublish(snapshot));
        Assert.Same(snapshot, publisher.Current);
    }

    [Theory]
    [InlineData(0)]  // equal version: stale
    [InlineData(-1)] // renumbered older snapshot: stale
    public void Stale_Versions_Are_Discarded_Never_Published(long staleVersionOffset)
    {
        var publisher = new ScenePublisher();
        var first = MakeSnapshot(5, 10);
        Assert.True(publisher.TryPublish(first));

        var stale = MakeSnapshot(5 + staleVersionOffset, 11);
        Assert.False(publisher.TryPublish(stale));
        Assert.Same(first, publisher.Current); // current snapshot untouched
    }

    [Fact]
    public void Newer_Version_Replaces_Current_Atomically()
    {
        var publisher = new ScenePublisher();
        var first = MakeSnapshot(1, 10);
        var second = MakeSnapshot(2, 20);
        publisher.TryPublish(first);
        publisher.TryPublish(second);
        Assert.Same(second, publisher.Current);
    }

    [Fact]
    public void Published_Event_Raises_With_The_Same_Instance_And_Exceptions_Are_Isolated()
    {
        var publisher = new ScenePublisher();
        SceneSnapshot? received = null;

        publisher.Published += _ => throw new InvalidOperationException("bad subscriber");
        publisher.Published += s => received = s;

        var snapshot = MakeSnapshot(1, 3);
        Assert.True(publisher.TryPublish(snapshot));
        Assert.Same(snapshot, received); // healthy subscriber still notified after bad one threw
    }

    [Fact]
    public async Task Concurrent_Publishers_Keep_Versions_Strictly_Monotonic_And_Readers_Never_See_Partial_Data()
    {
        var publisher = new ScenePublisher();

        // Writer convention: snapshot version V carries exactly V primitives, so a reader that
        // observes version V can verify it did NOT see a partially-built scene.
        const int writers = 8;
        const int versionsPerWriter = 50;

        var writerTasks = new System.Threading.Tasks.Task[writers];

        for (var w = 0; w < writers; w++)
        {
            var writer = w;
            // Odd writers publish their range in REVERSE order, guaranteeing out-of-order
            // arrivals and therefore stale discards (deterministic race coverage).
            var ascending = writer % 2 == 0;
            writerTasks[w] = System.Threading.Tasks.Task.Run(() =>
            {
                for (var k = 1; k <= versionsPerWriter; k++)
                {
                    var i = ascending ? k : versionsPerWriter + 1 - k;
                    var version = writer * versionsPerWriter + i;
                    publisher.TryPublish(MakeSnapshot(version, (int)version));
                }
            });
        }

        long lastSeenVersion = 0;
        var readerTask = System.Threading.Tasks.Task.Run(() =>
        {
            for (var probe = 0; probe < 2000; probe++)
            {
                var snapshot = publisher.Current;
                if (snapshot is null) continue;
                Assert.True(snapshot.Version >= lastSeenVersion, "Version went backwards for a reader.");
                lastSeenVersion = snapshot.Version;
                Assert.Equal(snapshot.Version, snapshot.TotalPrimitiveCount); // partial-scene sentinel
            }
        });

        await System.Threading.Tasks.Task.WhenAll(writerTasks);
        await readerTask;

        var final = publisher.Current;
        Assert.NotNull(final);
        Assert.Equal(writers * versionsPerWriter, final!.Version);
        Assert.Equal(writers * versionsPerWriter, final.TotalPrimitiveCount);

        // Deterministic stale check: once vMax is published, v1 can never win again.
        Assert.False(publisher.TryPublish(MakeSnapshot(1, 1)));
        Assert.Same(final, publisher.Current);
    }
}
