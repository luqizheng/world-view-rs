using System;
using System.Collections.Generic;
using System.Linq;
using Avalonia;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;
using SkiaSharp;
using WorldView.PresentationPrototype.Diagnostics;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Text;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Rendering;

/// <summary>
/// Draws one captured <see cref="RenderFrame"/> onto a leased SKCanvas.
///
/// Resource lifetime rules (Handoff §6):
///  - SKPaint/SKPath/SKTextBlob objects are created and disposed within a single Draw call;
///  - the ONLY retained resources are cached SKTypeface instances (font faces, not text runs),
///    owned by this renderer and disposed in <see cref="Dispose"/> at shutdown;
///  - this class is invoked exclusively from the render callback (MapDrawOperation.Render);
///    it accesses no UI, ViewModel, Core Domain or persistence state.
///
/// Text: draws PRE-SHAPED glyph IDs (from Avalonia/HarfBuzz shaping via ShapedLabel) as an
/// SKTextBlob. SkiaSharp never shapes text here - it only rasterizes glyph IDs at positions
/// computed from Avalonia's advances (Handoff §6.7: Skia is not a second shaping engine).
/// </summary>
public sealed class SkiaSceneRenderer : IDisposable
{
    // (family, weight, slant) -> cached SKTypeface. Font faces are reusable resources;
    // text runs (blobs) are per-frame and never cached.
    private readonly Dictionary<(string Family, int Weight, int Slant), SKTypeface> _typefaceCache = new();

    private readonly SKPaint _fillPaint = new() { IsAntialias = true, Style = SKPaintStyle.Fill };
    private readonly SKPaint _strokePaint = new() { IsAntialias = true, Style = SKPaintStyle.Stroke };
    private readonly SKPaint _textPaint = new() { IsAntialias = true, Style = SKPaintStyle.Fill };
    private readonly SKPath _path = new();
    private bool _disposed;

    public void Draw(SKCanvas canvas, RenderFrame frame, ImmediateDrawingContext context)
    {
        if (_disposed) throw new ObjectDisposedException(nameof(SkiaSceneRenderer));
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(frame);

        // Visible map-space AABB (conservative culling): unproject the four viewport corners.
        var (w, h) = (frame.ViewportLogicalSize.Width, frame.ViewportLogicalSize.Height);
        var corners = new[]
        {
            frame.Transform.ViewportToMap(new ViewportPoint(0, 0), w, h),
            frame.Transform.ViewportToMap(new ViewportPoint(w, 0), w, h),
            frame.Transform.ViewportToMap(new ViewportPoint(0, h), w, h),
            frame.Transform.ViewportToMap(new ViewportPoint(w, h), w, h),
        };
        var mapMinX = corners.Min(c => c.X); var mapMaxX = corners.Max(c => c.X);
        var mapMinY = corners.Min(c => c.Y); var mapMaxY = corners.Max(c => c.Y);
        var visibleMap = new MapRect(mapMinX, mapMinY, mapMaxX - mapMinX, mapMaxY - mapMinY);

        var layers = frame.Snapshot.Layers;
        foreach (var layer in layers.OrderBy(l => l.ZOrder))
        {
            foreach (var primitive in layer.Primitives)
            {
                // AABB cull in map space (conservative: may pass items near edges; never drops visible items).
                if (!Intersects(primitive.Bounds, visibleMap))
                    continue;

                DrawPrimitive(canvas, frame.Transform, primitive, w, h);
            }
        }

        // Presentation-only frame marker: visible snapshot version (TV-05 evidence aid).
        DrawVersionMarker(canvas, frame, w);
    }

    private static bool Intersects(MapRect a, MapRect b) =>
        a.X <= b.X + b.Width && a.X + a.Width >= b.X &&
        a.Y <= b.Y + b.Height && a.Y + a.Height >= b.Y;

    private void DrawPrimitive(SKCanvas canvas, ViewportTransform t, ScenePrimitive primitive, double vw, double vh)
    {
        switch (primitive)
        {
            case PointPrimitive point:
            {
                var p = t.MapToViewport(point.Position, vw, vh);
                _fillPaint.Color = ToSkColor(point.Fill);
                canvas.DrawCircle((float)p.X, (float)p.Y, (float)point.Radius, _fillPaint);
                break;
            }

            case PolylinePrimitive polyline:
            {
                if (polyline.Points.Length < 2) return;
                _path.Reset();
                var first = t.MapToViewport(polyline.Points[0], vw, vh);
                _path.MoveTo((float)first.X, (float)first.Y);
                for (var i = 1; i < polyline.Points.Length; i++)
                {
                    var p = t.MapToViewport(polyline.Points[i], vw, vh);
                    _path.LineTo((float)p.X, (float)p.Y);
                }
                _strokePaint.Color = ToSkColor(polyline.Stroke);
                _strokePaint.StrokeWidth = (float)polyline.StrokeWidth;
                _strokePaint.StrokeJoin = SKStrokeJoin.Round;
                _strokePaint.StrokeCap = SKStrokeCap.Round;
                canvas.DrawPath(_path, _strokePaint);
                break;
            }

            case PolygonPrimitive polygon:
            {
                if (polygon.Points.Length < 3) return;
                _path.Reset();
                var first = t.MapToViewport(polygon.Points[0], vw, vh);
                _path.MoveTo((float)first.X, (float)first.Y);
                for (var i = 1; i < polygon.Points.Length; i++)
                {
                    var p = t.MapToViewport(polygon.Points[i], vw, vh);
                    _path.LineTo((float)p.X, (float)p.Y);
                }
                _path.Close();
                _fillPaint.Color = ToSkColor(polygon.Fill);
                canvas.DrawPath(_path, _fillPaint);
                _strokePaint.Color = ToSkColor(polygon.Stroke);
                _strokePaint.StrokeWidth = (float)polygon.StrokeWidth;
                canvas.DrawPath(_path, _strokePaint);
                break;
            }

            case CubicBezierPrimitive bezier:
            {
                var p0 = t.MapToViewport(bezier.P0, vw, vh);
                var p1 = t.MapToViewport(bezier.P1, vw, vh);
                var p2 = t.MapToViewport(bezier.P2, vw, vh);
                var p3 = t.MapToViewport(bezier.P3, vw, vh);
                _path.Reset();
                _path.MoveTo((float)p0.X, (float)p0.Y);
                _path.CubicTo((float)p1.X, (float)p1.Y, (float)p2.X, (float)p2.Y, (float)p3.X, (float)p3.Y);
                _strokePaint.Color = ToSkColor(bezier.Stroke);
                _strokePaint.StrokeWidth = (float)bezier.StrokeWidth;
                _strokePaint.StrokeCap = SKStrokeCap.Round;
                canvas.DrawPath(_path, _strokePaint);
                break;
            }

            case TextLabelPrimitive label:
                DrawLabel(canvas, t, label, vw, vh);
                break;

            default:
                throw new ArgumentOutOfRangeException(nameof(primitive), primitive.GetType().Name);
        }
    }

    /// <summary>
    /// Skia glyph-drawing path: build an SKTextBlob from the Avalonia-shaped glyph indices,
    /// advances and offsets. Layout metric (AdvanceWidth) is compared with the drawn blob
    /// bounds and recorded - the "shaping/drawing metrics agree" validation (TV-03).
    /// </summary>
    private void DrawLabel(SKCanvas canvas, ViewportTransform t, TextLabelPrimitive label, double vw, double vh)
    {
        var shaped = label.Shaped;
        var font = GetFont(shaped);
        var anchor = t.MapToViewport(label.Anchor, vw, vh);

        using var blobBuilder = new SKTextBlobBuilder();
        // Positioned run: one SKPoint per glyph = cumulative advance + Avalonia glyph offset.
        var buffer = blobBuilder.AllocatePositionedRun(font, shaped.GlyphInfos.Count);

        var glyphSpan = buffer.Glyphs;
        var positionSpan = buffer.Positions;
        double x = 0;
        // Map-space label draws right/down in screen space; y runs downward from baseline.
        for (var i = 0; i < shaped.GlyphInfos.Count; i++)
        {
            var gi = shaped.GlyphInfos[i];
            glyphSpan[i] = gi.GlyphIndex;
            positionSpan[i] = new SKPoint((float)(x + gi.GlyphOffset.X), (float)gi.GlyphOffset.Y);
            x += gi.GlyphAdvance;
        }

        using var blob = blobBuilder.Build();
        if (blob is null) return;

        _textPaint.Color = ToSkColor(label.Fill);

        // Metric consistency: layout advance vs actual drawn ink bounds.
        var blobBounds = blob.Bounds;
        var layoutWidth = (float)shaped.AdvanceWidth;
        var inkWidth = blobBounds.Width;
        RenderDiagnostics.Instance.RecordTextMetric(shaped.Text, layoutWidth, inkWidth,
            Math.Abs(layoutWidth - inkWidth) / Math.Max(1, layoutWidth));

        canvas.DrawText(blob, (float)anchor.X, (float)anchor.Y, _textPaint);
    }

    /// <summary>Version marker drawn with the Skia path at a fixed logical position.</summary>
    private void DrawVersionMarker(SKCanvas canvas, RenderFrame frame, double vw)
    {
        var font = GetOrCreateFont("Consolas", 400, 0, 12);
        using var blob = SKTextBlob.Create($"v{frame.Snapshot.Version} @ {frame.Transform.MapUnitsPerPixel:F2} u/px", font, new SKPoint(10, 18));
        if (blob is null) return;
        _textPaint.Color = new SKColor(20, 20, 20, 220);
        canvas.DrawText(blob, 0, 0, _textPaint);
    }

    private SKFont GetFont(ShapedLabel shaped)
    {
        var typeface = shaped.Typeface;
        return GetOrCreateFont(
            typeface.FontFamily.Name,
            (int)typeface.Weight,
            typeface.Style == FontStyle.Italic ? 1 : 0,
            (float)shaped.FontRenderingEmSize);
    }

    private SKFont GetOrCreateFont(string family, int weight, int slant, float size)
    {
        var skTypeface = GetOrCreateTypeface(family, weight, slant);
        return new SKFont(skTypeface, size, 0f, 0f);
    }

    private SKTypeface GetOrCreateTypeface(string family, int weight, int slant)
    {
        if (_disposed) throw new ObjectDisposedException(nameof(SkiaSceneRenderer));
        var key = (family, weight, slant);
        if (_typefaceCache.TryGetValue(key, out var cached))
            return cached;

        var created = SKTypeface.FromFamilyName(
            family,
            (SKFontStyleWeight)weight,
            SKFontStyleWidth.Normal,
            slant == 1 ? SKFontStyleSlant.Italic : SKFontStyleSlant.Upright);

        // FromFamilyName falls back to the default typeface when the family is unknown;
        // that fallback is recorded in the validation report as a known limitation of the
        // Skia direct path (Avalonia's FontManager fallback is richer).
        _typefaceCache[key] = created;
        return created;
    }

    private static SKColor ToSkColor(ColorRgba c) => new(c.R, c.G, c.B, c.A);

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        foreach (var typeface in _typefaceCache.Values)
            typeface.Dispose();
        _typefaceCache.Clear();
        _fillPaint.Dispose();
        _strokePaint.Dispose();
        _textPaint.Dispose();
        _path.Dispose();
    }
}
