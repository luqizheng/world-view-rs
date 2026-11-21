using System;
using System.Threading;
using System.Threading.Tasks;
using WorldView.PresentationPrototype.Diagnostics;
using WorldView.PresentationPrototype.Scene;

namespace WorldView.PresentationPrototype.Rendering;

/// <summary>
/// Background scene-build worker (Handoff §7 "Background worker" row).
///
/// Builds complete immutable snapshots off the UI thread and publishes them through the
/// atomic <see cref="ScenePublisher"/>. Cancellation is cooperative: a cancelled build is
/// discarded BEFORE TryPublish, so partial data can never reach the render path.
/// The worker never touches Avalonia controls or UI state.
/// </summary>
public sealed class SceneBuildService : IDisposable
{
    private readonly ScenePublisher _publisher;
    private CancellationTokenSource? _cts = new();
    private Task? _currentBuild;
    private long _lastRequestedVersion;
    private bool _disposed;

    public SceneBuildService(ScenePublisher publisher)
    {
        _publisher = publisher ?? throw new ArgumentNullException(nameof(publisher));
    }

    /// <summary>Seconds of artificial build delay for the smallest fixtures, so snapshot
    /// replacement is observable by eye during the concurrency validation (TV-05).</summary>
    public TimeSpan SimulatedBuildDelay { get; set; } = TimeSpan.Zero;

    /// <summary>Queue a scene build with the given feature count. Supersedes any running build
    /// (its result is discarded as stale by version monotonicity or cancellation).</summary>
    public void RequestBuild(int featureCount, int seed)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);

        var version = Interlocked.Increment(ref _lastRequestedVersion);

        // Cancel the previous build; it either finishes and loses the version race
        // (TryPublish returns false) or observes cancellation and exits early.
        var fresh = new CancellationTokenSource();
        var oldCts = Interlocked.Exchange(ref _cts, fresh);
        if (oldCts is not null)
        {
            oldCts.Cancel();
            oldCts.Dispose();
        }

        var cts = fresh;
        var build = Task.Run(() =>
        {
            try
            {
                if (SimulatedBuildDelay > TimeSpan.Zero && featureCount <= 64)
                    Thread.Sleep(SimulatedBuildDelay);

                cts.Token.ThrowIfCancellationRequested();

                var snapshot = SceneFixtures.Build(featureCount, seed, version);

                cts.Token.ThrowIfCancellationRequested();

                // Publish only complete snapshots; stale versions are refused by the publisher.
                if (_publisher.TryPublish(snapshot))
                {
                    var diag = RenderDiagnostics.Instance;
                    diag.RecordSceneBuild(snapshot.BuildDiagnostics.BuildDuration.TotalMilliseconds);
                    diag.RecordFeatureCount(featureCount);
                }
            }
            catch (OperationCanceledException)
            {
                // Expected on supersede/shutdown: no partial publication (Handoff §5.4).
            }
        }, CancellationToken.None); // the task itself observes cts.Token internally

        // Keep the task referenced so shutdown can await it (no fire-and-forget leaks).
        var previous = Interlocked.Exchange(ref _currentBuild, build);
        previous?.ContinueWith(_ => { }, TaskScheduler.Default);
    }

    /// <summary>Stop outstanding work. Safe to call from UI thread during shutdown.</summary>
    public async Task ShutdownAsync()
    {
        var fresh = new CancellationTokenSource();
        var cts = Interlocked.Exchange(ref _cts, fresh);
        if (cts is not null)
        {
            cts.Cancel();
            cts.Dispose();
        }

        var build = Interlocked.Exchange(ref _currentBuild, null);
        if (build is not null)
        {
            try { await build.ConfigureAwait(false); }
            catch { /* build observes cancellation internally */ }
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        var cts = Interlocked.Exchange(ref _cts, null);
        cts?.Cancel();
        cts?.Dispose();
    }
}
