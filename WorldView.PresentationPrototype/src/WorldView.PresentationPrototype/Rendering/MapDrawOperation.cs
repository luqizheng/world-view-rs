using System;
using System.Diagnostics;
using Avalonia;
using Avalonia.Media;
using Avalonia.Rendering.SceneGraph;
using Avalonia.Skia;
using SkiaSharp;
using WorldView.PresentationPrototype.Diagnostics;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Rendering;

/// <summary>
/// Avalonia 12 custom draw operation (ICustomDrawOperation, Avalonia.Rendering.SceneGraph).
///
/// Contract enforcement points (Handoff §5.1, §6, §7):
///  - captures EXACTLY ONE snapshot reference at construction; the entire draw reads only that
///    snapshot - it never fetches geometry, labels or poses independently during rendering;
///  - receives an immutable RenderFrame (snapshot + camera + size + DPI); no control/ViewModel/
///    domain/persistence access anywhere in this class;
///  - Skia lease (canvas, paints) is acquired and disposed within Render() - nothing is retained
///    between callbacks;
///  - DPI scaling is applied EXACTLY ONCE, here, via canvas.Scale(scaling); everything inside
///    draws in viewport logical pixels;
///  - bounds are the viewport logical rect; hit-testing is coarse scene-graph behavior only -
///    real map hit testing belongs to Spatial Presentation and is out of TV-01 scope.
/// </summary>
public sealed class MapDrawOperation : ICustomDrawOperation
{
    private readonly RenderFrame _frame;
    private readonly SkiaSceneRenderer _renderer;
    private readonly RenderDiagnostics _diagnostics;

    public MapDrawOperation(RenderFrame frame, SkiaSceneRenderer renderer, RenderDiagnostics diagnostics)
    {
        _frame = frame ?? throw new ArgumentNullException(nameof(frame));
        _renderer = renderer ?? throw new ArgumentNullException(nameof(renderer));
        _diagnostics = diagnostics ?? throw new ArgumentNullException(nameof(diagnostics));
    }

    /// <summary>The single captured snapshot (evidence for Handoff acceptance criterion 7).</summary>
    public SceneSnapshot CapturedSnapshot => _frame.Snapshot;

    public Rect Bounds => _frame.LogicalBounds;

    /// <summary>Coarse scene-graph hit test only (bounds containment).</summary>
    public bool HitTest(Avalonia.Point p) => _frame.LogicalBounds.Contains(p);

    public bool Equals(ICustomDrawOperation? other) => ReferenceEquals(this, other);

    public void Dispose()
    {
        // No lease, canvas or paint objects are retained across callbacks; nothing to release here.
    }

    public void Render(ImmediateDrawingContext context)
    {
        var sw = Stopwatch.StartNew();

        // Version-consistency invariant: draw must observe a coherent single snapshot.
        var snapshot = _frame.Snapshot
            ?? throw new InvalidOperationException("MapDrawOperation requires a snapshot.");
        var drawnVersion = snapshot.Version;

        _diagnostics.RecordScaling(_frame.Scaling);
        _diagnostics.RecordMapUnitsPerPixel(_frame.Transform.MapUnitsPerPixel);
        _diagnostics.RecordDrawnSnapshot(drawnVersion);
        _diagnostics.RecordDrawnPrimitiveCount(snapshot.TotalPrimitiveCount);
        _diagnostics.RecordDrawThread(Environment.CurrentManagedThreadId);

        var leaseFeature = context.TryGetFeature(typeof(ISkiaSharpApiLeaseFeature))
            as ISkiaSharpApiLeaseFeature;

        if (leaseFeature is null)
        {
            // Non-Skia backend (e.g. headless software renderer without the lease feature):
            // record and draw nothing rather than crashing - validation runs on Skia backends.
            _diagnostics.RecordVersionConsistencyViolation("ISkiaSharpApiLeaseFeature unavailable on this backend.");
            _diagnostics.RecordFrame(sw.Elapsed.TotalMilliseconds);
            return;
        }

        // Lease lifetime == this callback only (Handoff §6.3, §6.6).
        using var lease = leaseFeature.Lease();
        var canvas = lease.SkCanvas;

        // Record once which Skia backend actually executes (GPU context vs CPU raster):
        // essential context for interpreting the performance observations.
        if (lease.GrContext is { } gr)
            _diagnostics.RecordBackendInfo($"GPU {gr.Backend}");
        else
            _diagnostics.RecordBackendInfo("CPU raster");

        canvas.Save();

        // ---- DPI boundary: applied exactly once. After this, the canvas is in VIEWPORT
        //      LOGICAL PIXELS and every coordinate below is logical until Restore. ----
        canvas.Scale((float)_frame.Scaling);

        // Viewport clipping in logical pixels (the map viewport must not bleed outside).
        canvas.ClipRect(new SKRect(
            (float)_frame.LogicalBounds.X,
            (float)_frame.LogicalBounds.Y,
            (float)_frame.LogicalBounds.Right,
            (float)_frame.LogicalBounds.Bottom));

        try
        {
            _renderer.Draw(canvas, _frame, context);
        }
        finally
        {
            canvas.Restore();
        }

        sw.Stop();
        _diagnostics.RecordFrame(sw.Elapsed.TotalMilliseconds);
    }
}
