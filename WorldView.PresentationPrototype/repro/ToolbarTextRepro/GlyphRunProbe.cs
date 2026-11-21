using System;
using System.Globalization;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Media.Immutable;
using Avalonia.Media.TextFormatting;

namespace ToolbarTextRepro;

/// <summary>
/// Experiment C: minimal custom control that draws the SAME 20 mixed-script labels the
/// prototype draws in its viewport, through <see cref="DrawingContext.DrawGlyphRun"/>.
/// Modes:
///   off        - draw nothing (control exists, render pass runs)
///   construct  - shape + build every GlyphRun, but do not draw any
///   manual     - draw hand-built GlyphRuns (same construction as the prototype)
///   textlayout - draw the GlyphRuns that Avalonia's TextLayout produced
/// Only the GlyphRun construction/consumption differs; buttons/theme/fonts/DPI stay fixed.
/// </summary>
public sealed class GlyphRunProbe : Control
{
    private string _mode = "off";

    public string Mode
    {
        get => _mode;
        set
        {
            if (_mode == value) return;
            _mode = value;
            InvalidateVisual();
        }
    }

    private const double FontSize = 14;

    private static readonly Typeface LabelTypeface = new("Segoe UI");
    private static readonly Typeface CjkTypeface = new("Microsoft YaHei UI");

    // The exact label texts used by WorldView.PresentationPrototype.Scene.SceneFixtures.
    private static readonly (string Text, bool Rtl)[] Labels =
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

    public override void Render(DrawingContext context)
    {
        base.Render(context);

        double x = 12;
        double y = 32;

        foreach (var (text, rtl) in Labels)
        {
            var typeface = RequiresCjkFont(text) ? CjkTypeface : LabelTypeface;
            var bidiLevel = (sbyte)(rtl ? 1 : 0);

            var run = BuildManualRun(text, typeface, bidiLevel);

            switch (Mode)
            {
                case "manual":
                    DrawRun(context, run, new Point(x, y));
                    break;
                case "textlayout":
                    DrawTextLayoutRun(context, text, typeface, bidiLevel, new Point(x, y));
                    break;
                case "construct":
                default:
                    // "off" / "construct": shape + construct happened above, nothing drawn.
                    break;
            }

            x += run.Bounds.Width + 18;
            if (x > 840) { x = 12; y += 26; }
        }
    }

    private static GlyphRun BuildManualRun(string text, Typeface typeface, sbyte bidiLevel)
    {
        if (!FontManager.Current.TryGetGlyphTypeface(typeface, out var glyphTypeface))
            throw new InvalidOperationException($"No glyph typeface for '{typeface.FontFamily.Name}'.");

        var options = new TextShaperOptions(
            glyphTypeface,
            FontSize,
            bidiLevel,
            CultureInfo.InvariantCulture);

        var shaped = TextShaper.Current.ShapeText(text, options);

        var glyphInfos = new GlyphInfo[shaped.Length];
        for (var i = 0; i < shaped.Length; i++)
            glyphInfos[i] = shaped[i];

        var metrics = glyphTypeface.Metrics;
        var baseline = new Point(0, metrics.Ascent * (FontSize / metrics.DesignEmHeight));

        // The exact hand construction used by WorldView.PresentationPrototype.TextShapingService.
        return new GlyphRun(glyphTypeface, FontSize, text.AsMemory(), glyphInfos, baseline, bidiLevel);
    }

    private static void DrawRun(DrawingContext context, GlyphRun run, Point origin)
    {
        using (context.PushTransform(Matrix.CreateTranslation(origin)))
            context.DrawGlyphRun(new ImmutableSolidColorBrush(Colors.DarkSlateBlue), run);
    }

    private static void DrawTextLayoutRun(
        DrawingContext context, string text, Typeface typeface, sbyte bidiLevel, Point origin)
    {
        var layout = new TextLayout(
            text,
            typeface,
            FontSize,
            new ImmutableSolidColorBrush(Colors.DarkSlateBlue),
            TextAlignment.Left,
            TextWrapping.NoWrap,
            TextTrimming.None,
            flowDirection: bidiLevel != 0 ? FlowDirection.RightToLeft : FlowDirection.LeftToRight,
            maxWidth: double.PositiveInfinity,
            maxHeight: double.PositiveInfinity);

        using (context.PushTransform(Matrix.CreateTranslation(origin)))
        {
            foreach (var line in layout.TextLines)
            {
                foreach (var run in line.TextRuns)
                {
                    if (run is ShapedTextRun shapedRun)
                        context.DrawGlyphRun(new ImmutableSolidColorBrush(Colors.DarkSlateBlue), shapedRun.GlyphRun);
                }
            }
        }
    }

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
}
