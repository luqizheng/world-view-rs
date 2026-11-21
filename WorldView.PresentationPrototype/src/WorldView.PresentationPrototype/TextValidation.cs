using System;
using Avalonia;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;
using WorldView.PresentationPrototype.Text;

namespace WorldView.PresentationPrototype;

/// <summary>
/// TV-01 acceptance: Latin, CJK, mixed Latin/CJK and RTL shaping results are recorded
/// at startup so shaping evidence exists even before the UI renders (Handoff §8 TV-03).
/// Called from Program.Main after AppBuilder setup, on the UI (platform) thread.
/// </summary>
public static class TextValidation
{
    public static void RunStartupShapingSuite()
    {
        // Resolve a typeface per sample via Avalonia font fallback (no hardcoded Windows-only
        // fonts here; the CJK/Arabic samples are matched by FontManager.TryMatchCharacter).
        var samples = new (string Text, sbyte BidiLevel)[]
        {
            ("The quick brown fox jumps over the lazy dog 0123456789", 0),      // Latin + digits
            ("圣临港白鹿原墨泽城", 0),                                           // CJK
            ("Old Deer Plain 白鹿原", 0),                                        // mixed Latin/CJK
            ("نهر الفضة", 1),                                                    // Arabic RTL
            ("مرحبا Mixed و这个世界", 1),                                        // RTL + CJK + Latin mixed
            ("café ñoño naïve", 0),                                              // Latin-1 accents
            ("école combining: é", 0),                                          // combining acute
        };

        foreach (var (text, bidi) in samples)
        {
            var typeface = TextShapingService.ResolveTypeface(text);
            var shaped = TextShapingService.Shape(text, typeface, 14, bidi);

            // Basic sanity: shaped glyphs exist and layout advance is positive.
            if (shaped.GlyphInfos.Count == 0 || shaped.AdvanceWidth <= 0)
                throw new InvalidOperationException(
                    $"Shaping sanity failed for '{text}': {shaped.GlyphInfos.Count} glyphs, width {shaped.AdvanceWidth}");
        }
    }
}
