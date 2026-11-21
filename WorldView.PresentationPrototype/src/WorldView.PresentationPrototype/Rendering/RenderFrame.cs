using System;
using Avalonia;
using Avalonia.Media;
using WorldView.PresentationPrototype.Scene;
using WorldView.PresentationPrototype.Viewport;

namespace WorldView.PresentationPrototype.Rendering;

/// <summary>
/// Immutable per-frame render input (minimal RenderFrame per Renderer Contract §"self-contained"):
/// exactly one captured snapshot + camera transform + viewport logical bounds + DPI scale.
/// The draw operation receives this record and reads NOTHING else: no controls, no ViewModels,
/// no live domain/persistence access (Handoff §5.1, §7).
/// </summary>
public sealed record RenderFrame(
    SceneSnapshot Snapshot,
    ViewportTransform Transform,
    Size ViewportLogicalSize,
    double Scaling,
    RenderDiagnosticsFrame? OverlayHint = null)
{
    /// <summary>Logical-pixel bounds of the map viewport (draw operation Bounds).</summary>
    public Rect LogicalBounds => new(0, 0, ViewportLogicalSize.Width, ViewportLogicalSize.Height);
}

/// <summary>Optional presentation-only overlay hint data (frame markers). TV-01 keeps this minimal.</summary>
public sealed record RenderDiagnosticsFrame(string VersionMarkerText);
