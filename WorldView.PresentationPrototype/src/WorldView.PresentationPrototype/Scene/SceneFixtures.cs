using System;
using System.Collections.Immutable;
using System.Diagnostics;
using Avalonia.Media;
using WorldView.PresentationPrototype.Text;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Scene;

/// <summary>
/// Deterministic synthetic scene fixtures for TV-01 (Handoff §8 TV-06).
/// These are TEST fixtures - they carry no Core Domain data and make no geographic claims.
///
/// Mixed primitive recipe per feature count (deterministic, seeded):
///   ~40% points, ~30% polylines, ~20% polygons, ~10% cubic Bézier curves,
///   plus one text label every 50 features covering Latin / CJK / mixed / RTL samples.
/// </summary>
public static class SceneFixtures
{
    /// <summary>Map/test space extent of the fixture world: [0, 4096) x [0, 4096).</summary>
    public const double WorldSize = 4096;

    /// <summary>Mixed-script label texts cycled through the scene (Handoff §3: Latin, CJK, mixed, RTL).</summary>
    public static readonly (string Text, bool Rtl)[] LabelSamples =
    {
        ("Hillview Abbey", false),
        ("圣临港 · Harbor of the Seal", false),
        ("白鹿原 Old Deer Plain", false),
        ("نهر الفضة Silver River", true),
        ("墨泽城", false),
        ("Rynhavn", false),
        ("세안성", false),
        ("铅谷 Mines of Lead Valley", false),
        ("ظفار Gate", true),
        ("澜溪镇 Liánxī Town", false),
    };

    private static readonly Typeface LabelTypeface = new("Segoe UI");
    private static readonly Typeface CjkTypeface = new("Microsoft YaHei UI");

    /// <summary>Build a complete immutable snapshot with <paramref name="featureCount"/> synthetic features.</summary>
    public static SceneSnapshot Build(int featureCount, int seed, long version)
    {
        if (featureCount < 1) throw new ArgumentOutOfRangeException(nameof(featureCount));
        var startedAt = DateTimeOffset.UtcNow;
        var sw = Stopwatch.StartNew();
        var rng = new Random(seed);

        // Layer 0: base geometry; Layer 1: labels (higher Z).
        var baseBuilder = ImmutableArray.CreateBuilder<ScenePrimitive>(featureCount);
        var labelBuilder = ImmutableArray.CreateBuilder<ScenePrimitive>(featureCount / 50 + 16);

        var shapes = TextShapingService.ResolveTypeface(LabelSamples[0].Text, CjkTypeface);

        for (var i = 0; i < featureCount; i++)
        {
            var kindRoll = rng.NextDouble();

            if (kindRoll < 0.40)
            {
                baseBuilder.Add(MakePoint(rng));
            }
            else if (kindRoll < 0.70)
            {
                baseBuilder.Add(MakePolyline(rng));
            }
            else if (kindRoll < 0.90)
            {
                baseBuilder.Add(MakePolygon(rng));
            }
            else
            {
                baseBuilder.Add(MakeBezier(rng));
            }

            if (i % 50 == 0)
            {
                var sample = LabelSamples[(i / 50) % LabelSamples.Length];
                var typeface = RequiresCjkFont(sample.Text) ? CjkTypeface : LabelTypeface;
                var shaped = TextShapingService.Shape(sample.Text, typeface, 14, sample.Rtl ? (sbyte)1 : (sbyte)0);
                var anchor = new MapPoint(rng.NextDouble() * (WorldSize - 200) + 20, rng.NextDouble() * (WorldSize - 100) + 50);
                labelBuilder.Add(new TextLabelPrimitive(anchor, shaped, new ColorRgba(30, 30, 40)));
            }
        }

        _ = shapes; // resolved early to warm font manager (kept for parity with UI sample labels)

        sw.Stop();

        var layers = ImmutableArray.Create(
            new RenderLayerSnapshot("base-geometry", 0, baseBuilder.ToImmutable()),
            new RenderLayerSnapshot("labels", 10, labelBuilder.ToImmutable()));

        var diagnostics = new SceneBuildDiagnostics(
            startedAt,
            sw.Elapsed,
            layers[0].Primitives.Length + layers[1].Primitives.Length,
            featureCount,
            Environment.CurrentManagedThreadId);

        return new SceneSnapshot(version, SceneContext.TestDefault((int)version), layers, diagnostics);
    }

    /// <summary>True when the text contains CJK codepoints (choose the CJK-capable typeface).</summary>
    private static bool RequiresCjkFont(string text)
    {
        for (var i = 0; i < text.Length; i++)
        {
            var c = char.ConvertToUtf32(text, i);
            if (char.IsHighSurrogate(text[i])) i++;
            if (c is >= 0x2E80 and <= 0x9FFF or >= 0xAC00 and <= 0xD7AF)
                return true;
        }
        return false;
    }

    private static PointPrimitive MakePoint(Random rng) =>
        new(
            new MapPoint(rng.NextDouble() * WorldSize, rng.NextDouble() * WorldSize),
            Radius: 1.5 + rng.NextDouble() * 4,
            Fill: RandomColor(rng));

    private static PolylinePrimitive MakePolyline(Random rng)
    {
        var count = rng.Next(2, 8);
        var builder = ImmutableArray.CreateBuilder<MapPoint>(count);
        double x = rng.NextDouble() * WorldSize;
        double y = rng.NextDouble() * WorldSize;
        for (var k = 0; k < count; k++)
        {
            builder.Add(new MapPoint(x, y));
            x += (rng.NextDouble() - 0.35) * 60;
            y += (rng.NextDouble() - 0.5) * 60;
            x = Math.Clamp(x, 0, WorldSize);
            y = Math.Clamp(y, 0, WorldSize);
        }
        return new PolylinePrimitive(builder.ToImmutable(), 1.5, RandomColor(rng));
    }

    private static PolygonPrimitive MakePolygon(Random rng)
    {
        var count = rng.Next(3, 9);
        var cx = rng.NextDouble() * WorldSize;
        var cy = rng.NextDouble() * WorldSize;
        var r = 8 + rng.NextDouble() * 40;
        var builder = ImmutableArray.CreateBuilder<MapPoint>(count);
        var phase = rng.NextDouble() * Math.PI * 2;
        for (var k = 0; k < count; k++)
        {
            var angle = phase + k * Math.PI * 2 / count + (rng.NextDouble() - 0.5) * 0.2;
            builder.Add(new MapPoint(
                Math.Clamp(cx + Math.Cos(angle) * r * (0.7 + rng.NextDouble() * 0.6), 0, WorldSize),
                Math.Clamp(cy + Math.Sin(angle) * r * (0.7 + rng.NextDouble() * 0.6), 0, WorldSize)));
        }
        return new PolygonPrimitive(builder.ToImmutable(), RandomColor(rng, 90), RandomColor(rng), 1.0);
    }

    private static CubicBezierPrimitive MakeBezier(Random rng)
    {
        var p0 = new MapPoint(rng.NextDouble() * WorldSize, rng.NextDouble() * WorldSize);
        var p3 = new MapPoint(
            Math.Clamp(p0.X + (rng.NextDouble() - 0.5) * 200, 0, WorldSize),
            Math.Clamp(p0.Y + (rng.NextDouble() - 0.5) * 200, 0, WorldSize));
        var p1 = new MapPoint(p0.X + (p3.X - p0.X) * rng.NextDouble(), p0.Y + rng.NextDouble() * 80 - 40);
        var p2 = new MapPoint(p3.X - (p3.X - p0.X) * rng.NextDouble(), p3.Y + rng.NextDouble() * 80 - 40);
        return new CubicBezierPrimitive(p0, p1, p2, p3, 1.2, RandomColor(rng));
    }

    private static ColorRgba RandomColor(Random rng, byte alpha = 200) => new(
        (byte)rng.Next(40, 230),
        (byte)rng.Next(40, 230),
        (byte)rng.Next(40, 230),
        alpha);
}
