using System;
using System.Globalization;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;

namespace ToolbarTextRepro;

/// <summary>
/// Copy of the prototype's startup shaping suite (TextValidation): shapes 7 samples and
/// builds a hand-constructed GlyphRun for each, but never draws them. Used to test whether
/// earlier GlyphRun construction is part of the corruption trigger.
/// </summary>
public static class ShapingSuite
{
    private const double FontSize = 14;

    public static void Run()
    {
        var samples = new (string Text, sbyte BidiLevel)[]
        {
            ("The quick brown fox jumps over the lazy dog 0123456789", 0),
            ("圣临港白鹿原墨泽城", 0),
            ("Old Deer Plain 白鹿原", 0),
            ("نهر الفضة", 1),
            ("مرحبا Mixed و这个世界", 1),
            ("café ñoño naïve", 0),
            ("école combining: é", 0),
        };

        foreach (var (text, bidi) in samples)
        {
            var typeface = ResolveTypeface(text);
            var run = BuildRun(text, typeface, bidi);
            if (run.GlyphInfos.Count == 0)
                throw new InvalidOperationException($"Shaping sanity failed for '{text}'.");
        }
    }

    private static Typeface ResolveTypeface(string text)
    {
        var baseFamily = FontManager.Current.DefaultFontFamily;
        var probe = text.Length == 0 ? 'A' : text[0];
        if (FontManager.Current.TryMatchCharacter(
                char.ConvertToUtf32(text, 0),
                FontStyle.Normal,
                FontWeight.Normal,
                FontStretch.Normal,
                baseFamily,
                CultureInfo.InvariantCulture,
                out var matched))
        {
            return matched;
        }
        return new Typeface(baseFamily);
    }

    private static GlyphRun BuildRun(string text, Typeface typeface, sbyte bidiLevel)
    {
        if (!FontManager.Current.TryGetGlyphTypeface(typeface, out var glyphTypeface))
            throw new InvalidOperationException($"No glyph typeface for '{typeface.FontFamily.Name}'.");

        var options = new TextShaperOptions(glyphTypeface, FontSize, bidiLevel, CultureInfo.InvariantCulture);
        var shaped = TextShaper.Current.ShapeText(text, options);

        var glyphInfos = new GlyphInfo[shaped.Length];
        for (var i = 0; i < shaped.Length; i++)
            glyphInfos[i] = shaped[i];

        var metrics = glyphTypeface.Metrics;
        var baseline = new Avalonia.Point(0, metrics.Ascent * (FontSize / metrics.DesignEmHeight));
        return new GlyphRun(glyphTypeface, FontSize, text.AsMemory(), glyphInfos, baseline, bidiLevel);
    }
}
