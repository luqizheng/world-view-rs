namespace WorldView.PresentationPrototype.Viewport;

// Explicit coordinate-space wrappers (Spatial Presentation & Geometry Interaction Contract §3.1).
// Four semantic spaces exist; TV-01 exercises three of them (Domain coordinates are out of scope):
//   Map/Test coordinates -> Viewport logical pixels -> Device physical pixels.
// The distinct record types exist so that mixing coordinate spaces is a compile-time error,
// never a silent numeric bug. No implicit conversions are defined anywhere in this file.

/// <summary>Position in map/test space (TV-01 synthetic scene space; NOT geographic CRS space).</summary>
public readonly record struct MapPoint(double X, double Y)
{
    public static readonly MapPoint Zero = default;
    public override string ToString() => $"Map({X:F2}, {Y:F2})";
}

/// <summary>A rectangle in map/test space.</summary>
public readonly record struct MapRect(double X, double Y, double Width, double Height)
{
    public bool Contains(MapPoint p) =>
        p.X >= X && p.X <= X + Width && p.Y >= Y && p.Y <= Y + Height;

    public MapPoint Center => new(X + Width / 2, Y + Height / 2);
    public static readonly MapRect Empty = default;
}

/// <summary>Position in viewport logical pixels (Avalonia layout space, pre-DPI).</summary>
public readonly record struct ViewportPoint(double X, double Y)
{
    public static readonly ViewportPoint Zero = default;
    public override string ToString() => $"Viewport({X:F1}, {Y:F1})";
}

/// <summary>A rectangle in viewport logical pixels.</summary>
public readonly record struct ViewportRect(double X, double Y, double Width, double Height)
{
    public bool Contains(ViewportPoint p) =>
        p.X >= X && p.X <= X + Width && p.Y >= Y && p.Y <= Y + Height;
}

/// <summary>Position in device physical pixels (renderer backend space only).</summary>
public readonly record struct DevicePoint(double X, double Y)
{
    public override string ToString() => $"Device({X:F1}, {Y:F1})";
}

/// <summary>RGBA color, backend-neutral. The scene model must not carry Skia/Avalonia brush types.</summary>
public readonly record struct ColorRgba(byte R, byte G, byte B, byte A = 255);
