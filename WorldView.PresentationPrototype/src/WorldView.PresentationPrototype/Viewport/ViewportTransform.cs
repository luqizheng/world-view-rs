using System;

namespace WorldView.PresentationPrototype.Viewport;

/// <summary>
/// Immutable camera transform: the only component allowed to convert between coordinate spaces.
/// Pipeline stages (each applied exactly once, in this order):
///   Map/Test coordinates -> ViewportTransform -> Viewport logical pixels
///   Viewport logical pixels -> (DPI scaling, applied exactly once) -> Device physical pixels.
///
/// Camera-only state deliberately lives OUTSIDE the scene snapshot: panning/zooming must not
/// bump SnapshotVersion or rebuild scene data (Renderer Contract &amp; Scene Model V0.1 §7).
/// Pan/zoom never mutates map coordinates; they are pure viewing parameters.
/// </summary>
public readonly record struct ViewportTransform(double MapUnitsPerPixel, MapPoint Center)
{
    /// <summary>Clamp range for zoom factor sanity; keeps MapUnitsPerPixel positive.</summary>
    public const double MinMapUnitsPerPixel = 1e-4;
    public const double MaxMapUnitsPerPixel = 1e6;

    public static readonly ViewportTransform Default = new(1.0, MapPoint.Zero);

    /// <summary>
    /// Map point -> viewport logical pixel. Viewport origin is top-left; the transform center
    /// is the map point displayed at the viewport center.
    /// </summary>
    public ViewportPoint MapToViewport(MapPoint p, double viewportWidth, double viewportHeight)
    {
        var centerX = viewportWidth / 2;
        var centerY = viewportHeight / 2;
        return new ViewportPoint(
            centerX + (p.X - Center.X) / MapUnitsPerPixel,
            centerY - (p.Y - Center.Y) / MapUnitsPerPixel); // map Y-up, screen Y-down
    }

    /// <summary>Viewport logical pixel -> map point (exact inverse of <see cref="MapToViewport"/>).</summary>
    public MapPoint ViewportToMap(ViewportPoint p, double viewportWidth, double viewportHeight)
    {
        var centerX = viewportWidth / 2;
        var centerY = viewportHeight / 2;
        return new MapPoint(
            Center.X + (p.X - centerX) * MapUnitsPerPixel,
            Center.Y - (p.Y - centerY) * MapUnitsPerPixel);
    }

    /// <summary>
    /// Viewport logical pixels -> device physical pixels. The DPI boundary.
    /// This is the ONLY place scaling is applied; renderers must not multiply by DPI anywhere else.
    /// </summary>
    public DevicePoint ToDevice(ViewportPoint p, double scaling) =>
        new(p.X * scaling, p.Y * scaling);

    /// <summary>Pan by a map-space delta (used by drag converted via current transform).</summary>
    public ViewportTransform Pan(MapPoint mapDelta) =>
        new(MapUnitsPerPixel, new MapPoint(Center.X - mapDelta.X, Center.Y - mapDelta.Y));

    /// <summary>Pan by viewport pixel delta (drag right moves map content right = center moves left in map space).</summary>
    public ViewportTransform PanPixels(ViewportPoint pixelDelta) =>
        new(MapUnitsPerPixel, new MapPoint(Center.X - pixelDelta.X * MapUnitsPerPixel, Center.Y + pixelDelta.Y * MapUnitsPerPixel));

    /// <summary>
    /// Zoom anchored at a map point: after zooming, the anchor map point stays at the same
    /// viewport pixel it occupied before. Pure function; returns a new transform.
    /// </summary>
    public ViewportTransform ZoomAt(MapPoint anchor, double factor, double viewportWidth, double viewportHeight)
    {
        var newScale = Math.Clamp(MapUnitsPerPixel / factor, MinMapUnitsPerPixel, MaxMapUnitsPerPixel);

        // Keep the anchor fixed on screen: the viewport pixel of the anchor under the current
        // transform must remain the viewport pixel of the anchor under the new transform.
        var before = MapToViewport(anchor, viewportWidth, viewportHeight);
        // Solve for new center C' such that MapToViewport(anchor | C', scale') == before.
        //   cx' + (ax - Cx')/s' == cx  =>  Cx' = (ax*s' - (cx - cx')*s') ... solve directly:
        //   viewportX = cx' + (ax - Cx')/s'  =>  Cx' = ax - (viewportX - cx') * s'
        var halfW = viewportWidth / 2;
        var halfH = viewportHeight / 2;
        var newCenterX = anchor.X - (before.X - halfW) * newScale;
        var newCenterY = anchor.Y + (before.Y - halfH) * newScale;
        return new ViewportTransform(newScale, new MapPoint(newCenterX, newCenterY));
    }
}
