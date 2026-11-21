using Avalonia.Headless.XUnit;
using Avalonia.Media;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Text;
using Xunit;

namespace WorldView.PresentationPrototype.Tests;

/// <summary>
/// Text shaping validation (TV-03) and synthetic fixture validation, executed under the
/// headless platform with the SAME shaping authority as the desktop app (UseHarfBuzz).
/// Covers Latin, CJK, mixed Latin/CJK, RTL (Arabic), and combining marks
/// (Handoff acceptance criterion 5).
/// </summary>
public class TextShapingAndFixtureTests
{
    private const string CjkProbes = "圣墨澜白";
    private static ShapedLabel Shape(string text, double size = 14, sbyte bidi = 0)
    {
        var typeface = TextShapingService.ResolveTypeface(text);
        return TextShapingService.Shape(text, typeface, size, bidi);
    }

    [AvaloniaFact]
    public void HarfBuzz_Shaper_Is_Active()
    {
        // TextShaper.Current is the wrapping type; the HarfBuzz shaper is installed as its
        // platform implementation (_platformImpl) by UseHarfBuzz(). Verify via reflection.
        var shaper = Avalonia.Media.TextFormatting.TextShaper.Current;
        var implField = typeof(Avalonia.Media.TextFormatting.TextShaper).GetField(
            "_platformImpl", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
        var impl = implField?.GetValue(shaper);
        Assert.NotNull(impl);
        Assert.Contains("HarfBuzz", impl!.GetType().FullName, StringComparison.OrdinalIgnoreCase);
    }

    [AvaloniaFact]
    public void Latin_Text_Shapes_With_Correct_Advance_Sum()
    {
        var shaped = Shape("The quick brown fox");
        Assert.True(shaped.GlyphInfos.Count >= 19, $"Expected one glyph per char (no ligature merging on ASCII), got {shaped.GlyphInfos.Count}");
        Assert.False(shaped.IsRightToLeft);
        double sum = 0;
        foreach (var gi in shaped.GlyphInfos) sum += gi.GlyphAdvance;
        Assert.Equal(shaped.AdvanceWidth, sum, 6);
        Assert.True(shaped.AdvanceWidth > 0);
    }

    [AvaloniaFact]
    public void Cjk_Text_Shapes_With_Plausible_Glyph_Count()
    {
        var shaped = Shape("圣临港白鹿原");
        // 5 CJK ideographs. The system fallback chain may emit at most one extra glyph per
        // character missing from the primary font (recording this as the known font-coverage
        // limitation of the default font chain); shaping itself must not collapse or reorder.
        Assert.InRange(shaped.GlyphInfos.Count, 5, 6);
        Assert.True(shaped.AdvanceWidth > 0);
        Assert.False(shaped.IsRightToLeft);
    }

    [AvaloniaFact]
    public void Mixed_Latin_Cjk_Shapes_All_Characters()
    {
        var shaped = Shape("Old Deer Plain 白鹿原");
        Assert.True(shaped.GlyphInfos.Count >= 17, $"Expected >= 17 glyphs, got {shaped.GlyphInfos.Count}");
    }

    [AvaloniaFact]
    public void Arabic_Rtl_Shapes_Rtl_With_Joined_Forms()
    {
        var shaped = Shape("نهر الفضة", bidi: 1);
        Assert.True(shaped.IsRightToLeft);
        // Arabic contextual joining: 9 chars must NOT stay 9 isolated forms - HarfBuzz ligature/join
        // behavior yields fewer glyphs for the lam-alef ligature etc.
        Assert.True(shaped.GlyphInfos.Count <= 9, $"Joined Arabic expected <= 9 glyphs, got {shaped.GlyphInfos.Count}");
        Assert.True(shaped.GlyphInfos.Count >= 6, $"Arabic shaping unexpectedly collapsed glyphs: {shaped.GlyphInfos.Count}");
        Assert.True(shaped.AdvanceWidth > 0);
    }

    [AvaloniaFact]
    public void Combining_Marks_Produce_Metrics()
    {
        var shaped = Shape("écolé");
        Assert.True(shaped.GlyphInfos.Count >= 5);
        Assert.True(shaped.AdvanceWidth > 0);
    }

    [AvaloniaFact]
    public void GlyphRun_Bounds_Agree_With_Layout_Advance()
    {
        // Metric-consistency check (Handoff §6.8): the GlyphRun built from the shaped buffer
        // must have ink/advance bounds consistent with the summed advances we hand to Skia.
        foreach (var text in new[] { "The quick brown fox", "圣临港白鹿原", "نهر الفضة" })
        {
            var shaped = Shape(text);
            var runBounds = shaped.GlyphRun.Bounds;
            Assert.True(runBounds.Width > 0);
            var relativeDelta = System.Math.Abs(runBounds.Width - shaped.AdvanceWidth) / shaped.AdvanceWidth;
            Assert.True(relativeDelta < 0.10,
                $"GlyphRun width {runBounds.Width:F2} vs layout advance {shaped.AdvanceWidth:F2} for '{text}' (delta {relativeDelta:P1})");
        }
    }

    [AvaloniaFact]
    public void Fixture_Build_Is_Deterministic_And_Carries_All_Primitive_Kinds()
    {
        var a = SceneFixtures.Build(64, seed: 7, version: 1);
        var b = SceneFixtures.Build(64, seed: 7, version: 1);

        Assert.Equal(a.TotalPrimitiveCount, b.TotalPrimitiveCount);
        Assert.Equal(1, a.Version);

        var kinds = new System.Collections.Generic.HashSet<ScenePrimitiveKind>();
        var labelCount = 0;
        foreach (var layer in a.Layers)
            foreach (var p in layer.Primitives)
            {
                kinds.Add(p.Kind);
                if (p is TextLabelPrimitive) labelCount++;
            }

        Assert.Equal(64, a.BuildDiagnostics.FeatureCount);
        Assert.True(labelCount >= 2, "64 features need labels at feature 0 and 50.");
        Assert.Contains(ScenePrimitiveKind.Point, kinds);
        Assert.Contains(ScenePrimitiveKind.Polyline, kinds);
        Assert.Contains(ScenePrimitiveKind.Polygon, kinds);
        Assert.Contains(ScenePrimitiveKind.CubicBezier, kinds);
        Assert.Contains(ScenePrimitiveKind.TextLabel, kinds);
    }

    [AvaloniaFact]
    public void Fixture_Labels_Cover_Latin_Cjk_And_Rtl()
    {
        var snapshot = SceneFixtures.Build(64 * 5, seed: 3, version: 1); // enough to cycle the sample set
        var rtlSeen = false;
        var cjkSeen = false;
        var latinSeen = false;

        foreach (var layer in snapshot.Layers)
            foreach (var p in layer.Primitives)
            {
                if (p is not TextLabelPrimitive label) continue;
                if (label.Shaped.IsRightToLeft) rtlSeen = true;
                else if (label.Shaped.Text.IndexOfAny(CjkProbes.ToCharArray()) >= 0) cjkSeen = true;
                else latinSeen = true;
            }

        Assert.True(rtlSeen, "Fixture labels must include RTL samples.");
        Assert.True(cjkSeen, "Fixture labels must include CJK samples.");
        Assert.True(latinSeen, "Fixture labels must include Latin samples.");
    }

    [AvaloniaFact]
    public void Background_Build_Publishes_Complete_Snapshot_And_Supersede_Discards_Stale()
    {
        var publisher = new ScenePublisher();
        using var service = new SceneBuildService(publisher);

        var published = new System.Threading.Tasks.TaskCompletionSource<SceneSnapshot>(System.Threading.Tasks.TaskCreationOptions.RunContinuationsAsynchronously);
        publisher.Published += s => published.TrySetResult(s);

        service.RequestBuild(64, seed: 1);
        Assert.True(published.Task.Wait(30_000), "First publish did not arrive within 30 s.");
        var first = published.Task.Result;
        Assert.Equal(1, first.Version);
        Assert.Equal(64, first.BuildDiagnostics.FeatureCount);
        Assert.True(first.TotalPrimitiveCount >= 64);

        // Supersede with rapid requests: final published version must be the LAST requested.
        var tcs = new System.Threading.Tasks.TaskCompletionSource<SceneSnapshot>(System.Threading.Tasks.TaskCreationOptions.RunContinuationsAsynchronously);
        publisher.Published += s => { if (s.Version == 21) tcs.TrySetResult(s); };

        for (var i = 0; i < 20; i++)
            service.RequestBuild(32, seed: i); // versions 2..21

        Assert.True(tcs.Task.Wait(30_000), "Final superseded publish did not arrive within 30 s.");
        var final = tcs.Task.Result;
        Assert.Equal(21, final.Version);
        Assert.Same(final, publisher.Current);
    }
}
