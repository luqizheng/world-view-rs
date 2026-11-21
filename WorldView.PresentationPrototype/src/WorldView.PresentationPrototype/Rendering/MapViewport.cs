using System;
using System.Threading;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Threading;
using WorldView.PresentationPrototype.Diagnostics;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Rendering;

/// <summary>
/// UI control hosting the map viewport. Owns camera state (UI thread), subscribes to the
/// ScenePublisher, and emits one MapDrawOperation per render pass - each carrying exactly one
/// captured snapshot reference plus the captured camera transform.
///
/// Threading (Handoff §7): input/layout on UI thread; publisher notifications are marshalled
/// via Dispatcher.UIThread.Post; the render path (MapDrawOperation/SkiaSceneRenderer) never
/// touches this control's state.
/// </summary>
public sealed class MapViewport : Control
{
    private readonly ViewportState _viewportState = new();
    private readonly SkiaSceneRenderer _renderer = new();
    private readonly RenderDiagnostics _diagnostics = RenderDiagnostics.Instance;
    private readonly ScenePublisher _publisher;

    // Pan gesture state (UI thread only).
    private ViewportPoint? _panLastPointer;
    private bool _panActive;

    public MapViewport(ScenePublisher publisher)
    {
        _publisher = publisher ?? throw new ArgumentNullException(nameof(publisher));
        _publisher.Published += OnPublished;
        Focusable = true;
    }

    /// <summary>Snapshot at frame-creation time; the draw op captures exactly this one.</summary>
    public SceneSnapshot? CurrentSnapshot => _publisher.Current;

    /// <summary>Current camera transform (for tests and diagnostics display).</summary>
    public ViewportTransform Transform => _viewportState.Transform;

    public SkiaSceneRenderer Renderer => _renderer;

    protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e)
    {
        _publisher.Published -= OnPublished;
        base.OnDetachedFromVisualTree(e);
    }

    private void OnPublished(SceneSnapshot snapshot)
    {
        // Publisher runs on a background build thread; marshal invalidation to the UI thread
        // WITHOUT blocking it (Post, never Invoke).
        Dispatcher.UIThread.Post(InvalidateVisual, DispatcherPriority.Background);
    }

    // No MeasureOverride: the viewport is a fill-control (desired size 0), it takes whatever
    // the hosting panel arranges. Returning availableSize here would corrupt Grid layout.

    protected override Size ArrangeOverride(Size finalSize)
    {
        // Layout drives the viewport logical size; DPI scaling comes from the visual root.
        _viewportState.SetViewportSize(finalSize.Width, finalSize.Height);
        _viewportState.SetScaling(LayoutHelper.GetRenderScaling(this));
        return base.ArrangeOverride(finalSize);
    }

    // ---------------- input -> camera transforms (UI thread only) ----------------

    protected override void OnPointerWheelChanged(PointerWheelEventArgs e)
    {
        var delta = e.Delta.Y;
        if (delta == 0) { base.OnPointerWheelChanged(e); return; }

        var factor = Math.Pow(1.2, Math.Clamp(delta, -3, 3));
        ZoomAtPointer(e.GetPosition(this), factor);
        e.Handled = true;
    }

    /// <summary>Zoom keeping the pointer's map point anchored at the same pixel.</summary>
    public void ZoomAtPointer(Avalonia.Point pointerPos, double factor)
    {
        var (w, h) = _viewportState.ViewportSize;
        var viewportPoint = new ViewportPoint(pointerPos.X, pointerPos.Y);
        var mapAnchor = _viewportState.Transform.ViewportToMap(viewportPoint, w, h);
        _viewportState.SetTransform(_viewportState.Transform.ZoomAt(mapAnchor, factor, w, h));
        InvalidateVisual();
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        if (e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            var pos = e.GetPosition(this);
            _panLastPointer = new ViewportPoint(pos.X, pos.Y);
            _panActive = true;
            e.Pointer.Capture(this);
            e.Handled = true;
        }
        base.OnPointerPressed(e);
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        if (!_panActive || _panLastPointer is null) return;

        var pos = e.GetPosition(this);
        var current = new ViewportPoint(pos.X, pos.Y);
        var delta = new ViewportPoint(current.X - _panLastPointer.Value.X, current.Y - _panLastPointer.Value.Y);
        _panLastPointer = current;

        _viewportState.SetTransform(_viewportState.Transform.PanPixels(delta));
        InvalidateVisual();
        e.Handled = true;
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        _panActive = false;
        _panLastPointer = null;
        e.Pointer.Capture(null);
        base.OnPointerReleased(e);
    }

    /// <summary>Reset camera to the default view covering the fixture world.</summary>
    public void ResetView()
    {
        var (w, h) = _viewportState.ViewportSize;
        // MapUnitsPerPixel = world units per pixel: the whole world must fit into the
        // smaller viewport dimension (with a small margin).
        var scale = SceneFixtures.WorldSize / Math.Min(w, h) * 0.95;
        _viewportState.SetTransform(new ViewportTransform(scale, new MapPoint(SceneFixtures.WorldSize / 2, SceneFixtures.WorldSize / 2)));
        InvalidateVisual();
    }

    // ---------------- render pass ----------------

    public override void Render(DrawingContext context)
    {
        // Single atomic read: this frame uses exactly the snapshot below, whatever publishes
        // next is for the NEXT frame. Camera transform captured as an immutable value.
        var snapshot = _publisher.Current;
        if (snapshot is null)
        {
            // No snapshot yet: presentation-only placeholder, not part of scene data.
            var hint = new FormattedText(
                "Building scene… (no snapshot published yet)",
                System.Globalization.CultureInfo.CurrentCulture,
                FlowDirection.LeftToRight,
                new Typeface(FontFamily.Default),
                13,
                Brushes.Gray);
            context.DrawText(hint, new Avalonia.Point(12, 24));
            return;
        }

        var (w, h) = _viewportState.ViewportSize;
        var frame = new RenderFrame(
            snapshot,
            _viewportState.Transform,
            new Size(w, h),
            _viewportState.Scaling);

        // Clear the viewport background (presentation-only), then the custom draw op.
        context.FillRectangle(Brushes.FloralWhite, frame.LogicalBounds);
        context.Custom(new MapDrawOperation(frame, _renderer, _diagnostics));

        // NOTE: the former DrawGlyphRun metric samples were removed from the production render
        // path (TV-01 follow-up). Drawing the hand-built GlyphRun produced by
        // TextShapingService.Shape with DrawingContext.DrawGlyphRun corrupted the text of other
        // controls in the same window. Glyph-run metrics stay covered by headless tests.
    }
}

/// <summary>DPI scale lookup (device pixels per logical pixel) from the visual root.</summary>
internal static class LayoutHelper
{
    public static double GetRenderScaling(Visual visual)
    {
        var root = TopLevel.GetTopLevel(visual);
        return root?.RenderScaling ?? 1.0;
    }
}
