namespace WorldView.PresentationPrototype.Viewport;

/// <summary>
/// UI-thread-owned mutable camera state. The render callback never reads this object
/// directly; the draw operation receives an immutable <see cref="ViewportTransform"/>
/// value captured at frame-creation time (see RenderFrame).
///
/// Thread discipline (Handoff §7):
///   allowed  : UI thread (input handlers, layout)
///   prohibited: background workers, render callback
/// </summary>
public sealed class ViewportState
{
    private ViewportTransform _transform = ViewportTransform.Default;
    private double _viewportWidth = 800;
    private double _viewportHeight = 600;
    private double _scaling = 1.0;

    /// <summary>Current immutable camera transform (copied value, safe to capture per frame).</summary>
    public ViewportTransform Transform => _transform;

    /// <summary>Current viewport logical size.</summary>
    public (double Width, double Height) ViewportSize => (_viewportWidth, _viewportHeight);

    /// <summary>Current DPI scaling factor (logical pixel -> device pixel).</summary>
    public double Scaling => _scaling;

    public void SetTransform(ViewportTransform transform)
    {
        _transform = transform;
    }

    public void SetViewportSize(double width, double height)
    {
        if (width > 0) _viewportWidth = width;
        if (height > 0) _viewportHeight = height;
    }

    public void SetScaling(double scaling)
    {
        if (scaling > 0) _scaling = scaling;
    }
}
