using System;
using System.Collections.Generic;
using Avalonia;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;

namespace WorldView.PresentationPrototype.Text;

/// <summary>
/// Immutable shaped-text payload produced by the ONE shaping authority:
/// Avalonia's configured shaping pipeline (Avalonia.HarfBuzz via TextShaper.Current).
///
/// Carries two consumers in TV-01:
///   1. the Skia direct path  - glyph indices + advances feed an SKTextBlob (Skia draws glyph IDs only,
///      it never re-shapes text);
///   2. the Avalonia path     - the captured <see cref="GlyphRun"/> is drawn via
///      ImmediateDrawingContext.DrawGlyphRun for metric cross-checking.
///
/// This is the "optional text/glyph presentation data required by the selected Avalonia drawing
/// route" explicitly permitted by Handoff §5.1. Holding the Avalonia GlyphTypeface here is a
/// TV-01 scope decision: a production scene model would carry a font resource handle instead.
/// GlyphRun instances are created once (on the shaping thread) and are read-only afterwards;
/// they must not be mutated after the label becomes part of a published snapshot.
/// </summary>
public sealed class ShapedLabel
{
    public ShapedLabel(
        string text,
        Typeface typeface,
        double fontRenderingEmSize,
        sbyte bidiLevel,
        IReadOnlyList<GlyphInfo> glyphInfos,
        double advanceWidth,
        double lineHeight,
        Point baselineOrigin,
        GlyphRun glyphRun)
    {
        if (glyphInfos is null || glyphInfos.Count == 0)
            throw new ArgumentException("Shaped label requires at least one glyph.", nameof(glyphInfos));

        Text = text;
        Typeface = typeface;
        FontRenderingEmSize = fontRenderingEmSize;
        BiDiLevel = bidiLevel;
        GlyphInfos = glyphInfos;
        AdvanceWidth = advanceWidth;
        LineHeight = lineHeight;
        BaselineOrigin = baselineOrigin;
        GlyphRun = glyphRun;
        IsRightToLeft = (bidiLevel & 1) != 0;
    }

    public string Text { get; }
    public Typeface Typeface { get; }
    public double FontRenderingEmSize { get; }
    public sbyte BiDiLevel { get; }
    public IReadOnlyList<GlyphInfo> GlyphInfos { get; }
    public bool IsRightToLeft { get; }

    /// <summary>Total advance width (layout metric, logical units at the given em size).</summary>
    public double AdvanceWidth { get; }

    /// <summary>Line height from font metrics (Ascent - Descent + LineGap, scaled to em size).</summary>
    public double LineHeight { get; }

    /// <summary>Baseline origin relative to the label's top-left, from GlyphRun layout.</summary>
    public Point BaselineOrigin { get; }

    /// <summary>Avalonia GlyphRun built from the same shaping result (used by DrawGlyphRun path).</summary>
    public GlyphRun GlyphRun { get; }

    public override string ToString() =>
        $"{(IsRightToLeft ? "RTL" : "LTR")} \"{(Text.Length <= 24 ? Text : Text[..24] + "…")}\" glyphs={GlyphInfos.Count} w={AdvanceWidth:F1}";
}
