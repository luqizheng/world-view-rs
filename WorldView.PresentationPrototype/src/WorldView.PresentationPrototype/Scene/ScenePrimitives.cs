using System;
using System.Collections.Immutable;
using WorldView.PresentationPrototype.Text;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Scene;

/// <summary>
/// Stable per-primitive identifier (Renderer Contract: RenderItemId).
/// Presentation-only identifier; must never be written back as a Core Domain Object ID.
/// </summary>
public readonly record struct RenderItemId(Guid Value)
{
    public static RenderItemId New() => new(Guid.NewGuid());
    public override string ToString() => Value.ToString("N")[..8];
}

/// <summary>Explicit presentation-only provenance (selection rings, labels, diagnostics). 
/// Maps to the contract's presentation-only category: these items never come from domain data.</summary>
public enum ScenePrimitiveKind : byte
{
    Point,
    Polyline,
    Polygon,
    CubicBezier,
    TextLabel,
}

/// <summary>
/// Base of all backend-neutral scene primitives. Coordinates are Map/Test space
/// (<see cref="MapPoint"/>) - NEVER device pixels (Handoff §5.2).
/// </summary>
public abstract record ScenePrimitive
{
    protected ScenePrimitive(RenderItemId id, ScenePrimitiveKind kind, MapRect bounds)
    {
        Id = id;
        Kind = kind;
        Bounds = bounds;
    }

    public RenderItemId Id { get; }
    public ScenePrimitiveKind Kind { get; }

    /// <summary>Conservative bounds in map space, computed at build time.</summary>
    public MapRect Bounds { get; }

    /// <summary>Set on presentation-only items (diagnostics markers etc.).</summary>
    public bool IsPresentationOnly { get; init; }
}

public sealed record PointPrimitive(MapPoint Position, double Radius, ColorRgba Fill) 
    : ScenePrimitive(NewId(), ScenePrimitiveKind.Point, ComputeBounds(Position, Radius))
{
    private static RenderItemId NewId() => RenderItemId.New();

    private static MapRect ComputeBounds(MapPoint p, double radius) =>
        new(p.X - radius, p.Y - radius, radius * 2, radius * 2);
}

public sealed record PolylinePrimitive(ImmutableArray<MapPoint> Points, double StrokeWidth, ColorRgba Stroke)
    : ScenePrimitive(RenderItemId.New(), ScenePrimitiveKind.Polyline, ComputeBounds(Points, StrokeWidth))
{
    private static MapRect ComputeBounds(ImmutableArray<MapPoint> points, double strokeWidth)
    {
        var minX = double.MaxValue; var minY = double.MaxValue;
        var maxX = double.MinValue; var maxY = double.MinValue;
        foreach (var p in points)
        {
            if (p.X < minX) minX = p.X;
            if (p.Y < minY) minY = p.Y;
            if (p.X > maxX) maxX = p.X;
            if (p.Y > maxY) maxY = p.Y;
        }
        var pad = strokeWidth / 2;
        return new MapRect(minX - pad, minY - pad, maxX - minX + strokeWidth, maxY - minY + strokeWidth);
    }
}

public sealed record PolygonPrimitive(ImmutableArray<MapPoint> Points, ColorRgba Fill, ColorRgba Stroke, double StrokeWidth)
    : ScenePrimitive(RenderItemId.New(), ScenePrimitiveKind.Polygon, ComputeBounds(Points, StrokeWidth))
{
    private static MapRect ComputeBounds(ImmutableArray<MapPoint> points, double strokeWidth)
    {
        var minX = double.MaxValue; var minY = double.MaxValue;
        var maxX = double.MinValue; var maxY = double.MinValue;
        foreach (var p in points)
        {
            if (p.X < minX) minX = p.X;
            if (p.Y < minY) minY = p.Y;
            if (p.X > maxX) maxX = p.X;
            if (p.Y > maxY) maxY = p.Y;
        }
        var pad = strokeWidth / 2;
        return new MapRect(minX - pad, minY - pad, maxX - minX + strokeWidth, maxY - minY + strokeWidth);
    }
}

public sealed record CubicBezierPrimitive(
    MapPoint P0, MapPoint P1, MapPoint P2, MapPoint P3,
    double StrokeWidth, ColorRgba Stroke)
    : ScenePrimitive(RenderItemId.New(), ScenePrimitiveKind.CubicBezier, ComputeBounds(P0, P1, P2, P3, StrokeWidth))
{
    // Conservative bound: convex hull of the four control points fully contains a cubic segment.
    private static MapRect ComputeBounds(MapPoint p0, MapPoint p1, MapPoint p2, MapPoint p3, double strokeWidth)
    {
        var xs = new[] { p0.X, p1.X, p2.X, p3.X };
        var ys = new[] { p0.Y, p1.Y, p2.Y, p3.Y };
        var minX = Math.Min(Math.Min(xs[0], xs[1]), Math.Min(xs[2], xs[3]));
        var maxX = Math.Max(Math.Max(xs[0], xs[1]), Math.Max(xs[2], xs[3]));
        var minY = Math.Min(Math.Min(ys[0], ys[1]), Math.Min(ys[2], ys[3]));
        var maxY = Math.Max(Math.Max(ys[0], ys[1]), Math.Max(ys[2], ys[3]));
        var pad = strokeWidth / 2;
        return new MapRect(minX - pad, minY - pad, maxX - minX + strokeWidth, maxY - minY + strokeWidth);
    }
}

/// <summary>TextLabelPrimitive carries pre-shaped immutable text (single shaping authority). 
/// Anchored at a map-space point; the anchor is the label's baseline-left.</summary>
public sealed record TextLabelPrimitive(MapPoint Anchor, ShapedLabel Shaped, ColorRgba Fill)
    : ScenePrimitive(RenderItemId.New(), ScenePrimitiveKind.TextLabel, ComputeBounds(Anchor, Shaped))
{
    private static MapRect ComputeBounds(MapPoint anchor, ShapedLabel shaped)
    {
        // Map Y grows up; label extends right and down from baseline in screen terms.
        return new MapRect(anchor.X, anchor.Y - shaped.LineHeight, shaped.AdvanceWidth, shaped.LineHeight);
    }
}
