using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Threading;

namespace WorldView.PresentationPrototype.Diagnostics;

/// <summary>
/// Thread-safe diagnostic counters shared by UI thread, background builders and the render
/// callback. This object is pure state; it never touches UI controls. The UI polls it on a
/// timer instead of the render callback pushing updates (Handoff §7: no UI access from render).
/// </summary>
public sealed class RenderDiagnostics
{
    public static RenderDiagnostics Instance { get; } = new();

    // ---- snapshot publication ----
    private long _publishedVersion;
    public long PublishedVersion => Volatile.Read(ref _publishedVersion);
    public void RecordPublish(long version) => Interlocked.Exchange(ref _publishedVersion, version);

    private long _discardedVersionCount;
    public long DiscardedVersionCount => Interlocked.Read(ref _discardedVersionCount);
    public void RecordDiscardedPublish() => Interlocked.Increment(ref _discardedVersionCount);

    // ---- scene build ----
    private double _lastSceneBuildMs;
    public double LastSceneBuildMs => Volatile.Read(ref _lastSceneBuildMs);
    public void RecordSceneBuild(double ms) => Volatile.Write(ref _lastSceneBuildMs, ms);

    private long _lastFeatureCount;
    public long LastFeatureCount => Interlocked.Read(ref _lastFeatureCount);
    public void RecordFeatureCount(long count) => Interlocked.Exchange(ref _lastFeatureCount, count);

    // ---- frame timing ----
    private double _lastFrameMs;
    public double LastFrameMs => Volatile.Read(ref _lastFrameMs);
    private double _avgFrameMs;
    public double AvgFrameMs => Volatile.Read(ref _avgFrameMs);
    private long _frameCount;
    public long FrameCount => Interlocked.Read(ref _frameCount);
    private double _worstFrameMs;
    public double WorstFrameMs => Volatile.Read(ref _worstFrameMs);

    public void RecordFrame(double ms)
    {
        var n = Interlocked.Increment(ref _frameCount);
        Volatile.Write(ref _lastFrameMs, ms);
        Volatile.Write(ref _avgFrameMs, Volatile.Read(ref _avgFrameMs) * (n - 1) / n + ms / n);
        // racy max is acceptable for diagnostics
        if (ms > Volatile.Read(ref _worstFrameMs)) Volatile.Write(ref _worstFrameMs, ms);
    }

    /// <summary>Reset per-phase frame statistics (used by the auto benchmark between phases).</summary>
    public void ResetPhaseCounters()
    {
        Volatile.Write(ref _frameCount, 0);
        Volatile.Write(ref _avgFrameMs, 0);
        Volatile.Write(ref _worstFrameMs, 0);
        Volatile.Write(ref _lastFrameMs, 0);
    }

    // ---- last draw context (captured per frame for version-consistency evidence) ----
    private long _lastDrawnSnapshotVersion;
    public long LastDrawnSnapshotVersion => Volatile.Read(ref _lastDrawnSnapshotVersion);
    public void RecordDrawnSnapshot(long version) => Interlocked.Exchange(ref _lastDrawnSnapshotVersion, version);

    private long _lastDrawnPrimitiveCount;
    public long LastDrawnPrimitiveCount => Volatile.Read(ref _lastDrawnPrimitiveCount);
    public void RecordDrawnPrimitiveCount(long count) => Volatile.Write(ref _lastDrawnPrimitiveCount, count);

    private long _lastDrawThread;
    /// <summary>Managed thread id of the last render callback invocation.</summary>
    public long LastDrawThread => Volatile.Read(ref _lastDrawThread);
    public void RecordDrawThread(long threadId) => Volatile.Write(ref _lastDrawThread, threadId);

    // ---- viewport ----
    private double _lastScaling;
    public double LastScaling => Volatile.Read(ref _lastScaling);
    public void RecordScaling(double s) => Volatile.Write(ref _lastScaling, s);

    // ---- render backend ----
    private string? _backendInfo;
    public string BackendInfo => Volatile.Read(ref _backendInfo) ?? "unknown";
    public void RecordBackendInfo(string info) => Volatile.Write(ref _backendInfo, info);

    private double _lastMapUnitsPerPixel;
    public double LastMapUnitsPerPixel => Volatile.Read(ref _lastMapUnitsPerPixel);
    public void RecordMapUnitsPerPixel(double v) => Volatile.Write(ref _lastMapUnitsPerPixel, v);

    // ---- process memory ----
    public long ManagedMemoryBytes => GC.GetTotalMemory(forceFullCollection: false);
    public long WorkingSetBytes => Process.GetCurrentProcess().WorkingSet64;

    // ---- shaping ----
    private long _shapedTextCount;
    public long ShapedTextCount => Interlocked.Read(ref _shapedTextCount);
    private readonly ConcurrentDictionary<string, byte> _shapedSample = new();
    public string LastShapedSample => Volatile.Read(ref _lastShapedSample) ?? string.Empty;
    private string? _lastShapedSample;
    public void RecordShaped(string text)
    {
        Interlocked.Increment(ref _shapedTextCount);
        Volatile.Write(ref _lastShapedSample, text);
    }

    // ---- text metric consistency (layout advance vs Skia-drawn ink bounds) ----
    private double _lastTextLayoutWidth;
    private double _lastTextInkWidth;
    private double _lastTextRelativeDelta;
    private string? _lastTextSample;
    private long _textMetricChecks;
    public long TextMetricChecks => Interlocked.Read(ref _textMetricChecks);
    public (string Sample, double LayoutWidth, double InkWidth, double RelativeDelta) LastTextMetric =>
        (Volatile.Read(ref _lastTextSample) ?? string.Empty,
         Volatile.Read(ref _lastTextLayoutWidth),
         Volatile.Read(ref _lastTextInkWidth),
         Volatile.Read(ref _lastTextRelativeDelta));
    public void RecordTextMetric(string sample, double layoutWidth, double inkWidth, double relativeDelta)
    {
        Interlocked.Increment(ref _textMetricChecks);
        Volatile.Write(ref _lastTextSample, sample);
        Volatile.Write(ref _lastTextLayoutWidth, layoutWidth);
        Volatile.Write(ref _lastTextInkWidth, inkWidth);
        Volatile.Write(ref _lastTextRelativeDelta, relativeDelta);
    }

    // ---- consistency violations detected at draw time (must stay zero) ----
    private long _versionConsistencyViolations;
    public long VersionConsistencyViolations => Interlocked.Read(ref _versionConsistencyViolations);
    public void RecordVersionConsistencyViolation(string detail)
    {
        Interlocked.Increment(ref _versionConsistencyViolations);
        Volatile.Write(ref _lastViolationDetail, detail);
    }
    private string? _lastViolationDetail;
    public string LastViolationDetail => Volatile.Read(ref _lastViolationDetail) ?? string.Empty;

    public string FormatSummary()
    {
        var memMb = WorkingSetBytes / 1024 / 1024;
        var gcMb = ManagedMemoryBytes / 1024 / 1024;
        return
            $"snapshot: published v{PublishedVersion} (discarded {DiscardedVersionCount}) | drawn v{LastDrawnSnapshotVersion} [{LastDrawnPrimitiveCount} prims]\n" +
            $"frames: {FrameCount} | last {LastFrameMs:F1} ms | avg {AvgFrameMs:F1} ms | worst {WorstFrameMs:F1} ms | draw thread {LastDrawThread}\n" +
            $"build: last scene {LastSceneBuildMs:F1} ms | features {LastFeatureCount} | shaped {ShapedTextCount} labels\n" +
            $"viewport: DPI x{LastScaling:F2} | {LastMapUnitsPerPixel:F2} map-unit/px | backend {BackendInfo} | mem {memMb} MB (GC {gcMb} MB)" +
            (VersionConsistencyViolations == 0 ? "" : $"\nVIOLATIONS: {VersionConsistencyViolations} - {LastViolationDetail}");
    }
}
