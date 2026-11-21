using System;
using System.Globalization;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;
using WorldView.PresentationPrototype.Diagnostics;

namespace WorldView.PresentationPrototype.Text;

/// <summary>
/// Single text-shaping entry point for TV-01.
///
/// Shaping authority: Avalonia's configured shaping pipeline. The application enables
/// Avalonia.HarfBuzz (Program.cs: UseHarfBuzz), which installs a HarfBuzz-backed TextShaper
/// into TextShaper.Current. This service therefore shapes through HarfBuzz via Avalonia -
/// SkiaSharp is never asked to shape text anywhere in this prototype (Skia only draws
/// already-shaped glyph IDs).
///
/// All shaping happens on background build threads during scene construction; the render
/// callback only consumes the immutable <see cref="ShapedLabel"/> payloads.
/// </summary>
public static class TextShapingService
{
    /// <summary>
    /// Serializes all access to Avalonia's text services (TextShaper.Current, FontManager).
    /// TV-01 finding: under Avalonia 12.1.6 + HarfBuzz these services are NOT thread-safe -
    /// concurrent shaping from background build threads while the UI thread renders text
    /// corrupts shared glyph caches, which manifests as UI text drawn with wrong glyphs
    /// (see validation report, "threaded shaping" finding). Scene building stays parallel;
    /// only the shaping section is serialized.
    /// </summary>
    private static readonly object ShapingLock = new();

    /// <summary>
    /// Shape <paramref name="text"/> with the given typeface at the given size.
    /// </summary>
    /// <param name="bidiLevel">0 for LTR runs, 1 for RTL runs (base direction).</param>
    public static ShapedLabel Shape(string text, Typeface typeface, double size, sbyte bidiLevel = 0)
    {
        if (string.IsNullOrEmpty(text))
            throw new ArgumentException("Text must be non-empty.", nameof(text));

        RenderDiagnostics.Instance.RecordShaped(text);

        lock (ShapingLock)
        {
            if (!FontManager.Current.TryGetGlyphTypeface(typeface, out var glyphTypeface))
                throw new InvalidOperationException($"No glyph typeface found for '{typeface.FontFamily.Name}'.");

            var options = new TextShaperOptions(
                glyphTypeface,
                size,
                bidiLevel,
                CultureInfo.InvariantCulture);

            var shaped = TextShaper.Current.ShapeText(text, options);

            // Copy the immutable glyph data out; keep the GlyphRun alive for the DrawGlyphRun path.
            var glyphInfos = new GlyphInfo[shaped.Length];
            double advanceWidth = 0;
            for (var i = 0; i < shaped.Length; i++)
            {
                glyphInfos[i] = shaped[i];
                advanceWidth += shaped[i].GlyphAdvance;
            }

            // Line height from the typeface metrics, scaled from design units to the em size.
            var metrics = glyphTypeface.Metrics;
            var scale = size / metrics.DesignEmHeight;
            var lineHeight = (metrics.Ascent - metrics.Descent + metrics.LineGap) * scale;

            var baselineOrigin = new Avalonia.Point(0, metrics.Ascent * scale);

            var glyphRun = new GlyphRun(
                glyphTypeface,
                size,
                text.AsMemory(),
                glyphInfos,
                baselineOrigin,
                bidiLevel);

            return new ShapedLabel(
                text,
                typeface,
                size,
                bidiLevel,
                glyphInfos,
                advanceWidth,
                lineHeight,
                baselineOrigin,
                glyphRun);
        }
    }

    /// <summary>
    /// Resolve a typeface that actually contains glyphs for the first codepoint of the text.
    /// Uses Avalonia's font fallback (FontManager.TryMatchCharacter) so CJK/Arabic samples get
    /// a font that renders on any platform, instead of hardcoding Windows font names.
    /// </summary>
    public static Typeface ResolveTypeface(string text, Typeface fallback = default)
    {
        var baseFamily = fallback == default
            ? FontManager.Current.DefaultFontFamily
            : fallback.FontFamily;

        lock (ShapingLock)
        {
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
    }
}
