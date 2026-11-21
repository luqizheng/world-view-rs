using WorldView.PresentationPrototype.Viewport;
using Xunit;

namespace WorldView.PresentationPrototype.Tests;

/// <summary>
/// Coordinate pipeline tests (TV-04): map -> viewport -> device conversions, zoom anchoring,
/// pan, and the exactly-once DPI rule (Handoff acceptance criterion 4).
/// </summary>
public class ViewportTransformTests
{
    private static readonly ViewportTransform T = new(MapUnitsPerPixel: 2.0, Center: new MapPoint(1000, 500));

    [Theory]
    [InlineData(0, 0)]
    [InlineData(400, 300)]
    [InlineData(799, 599)]
    [InlineData(-50, -40)]
    public void MapToViewport_Is_Inverted_By_ViewportToMap(double vx, double vy)
    {
        const double w = 800, h = 600;
        var vp = new ViewportPoint(vx, vy);
        var map = T.ViewportToMap(vp, w, h);
        var back = T.MapToViewport(map, w, h);
        Assert.InRange(back.X, vp.X - 1e-9, vp.X + 1e-9);
        Assert.InRange(back.Y, vp.Y - 1e-9, vp.Y + 1e-9);
    }

    [Fact]
    public void Center_Of_Viewport_Maps_To_Transform_Center()
    {
        const double w = 800, h = 600;
        var center = T.MapToViewport(T.Center, w, h);
        Assert.Equal(w / 2, center.X, 9);
        Assert.Equal(h / 2, center.Y, 9);
    }

    [Fact]
    public void Map_Y_Axis_Points_Up_Screen_Y_Axis_Points_Down()
    {
        const double w = 800, h = 600;
        var above = T.MapToViewport(new MapPoint(T.Center.X, T.Center.Y + 100), w, h);
        var below = T.MapToViewport(new MapPoint(T.Center.X, T.Center.Y - 100), w, h);
        var center = T.MapToViewport(T.Center, w, h);
        Assert.True(above.Y < center.Y, "Higher map Y must render above center on screen.");
        Assert.True(below.Y > center.Y, "Lower map Y must render below center on screen.");
    }

    [Theory]
    [InlineData(1.0, 1.0)]
    [InlineData(1.25, 1.25)]
    [InlineData(1.5, 1.5)]
    [InlineData(2.0, 2.0)]
    public void Device_Point_Is_Viewport_Point_Times_Scaling_Exactly_Once(double scaling, double expectedFactor)
    {
        var vp = new ViewportPoint(123.4, 567.8);
        var device = T.ToDevice(vp, scaling);
        Assert.Equal(vp.X * expectedFactor, device.X, 6);
        Assert.Equal(vp.Y * expectedFactor, device.Y, 6);
    }

    [Fact]
    public void Zoom_Keeps_Anchor_At_The_Same_Viewport_Pixel()
    {
        const double w = 800, h = 600;
        var anchor = new MapPoint(1010, 520); // near center
        var before = T.MapToViewport(anchor, w, h);

        foreach (var factor in new[] { 0.5, 1.2, 2.0, 10.0 })
        {
            var zoomed = T.ZoomAt(anchor, factor, w, h);
            var after = zoomed.MapToViewport(anchor, w, h);
            Assert.InRange(after.X, before.X - 1e-9, before.X + 1e-9);
            Assert.InRange(after.Y, before.Y - 1e-9, before.Y + 1e-9);
        }
    }

    [Fact]
    public void Zoom_In_Halves_Map_Units_Per_Pixel()
    {
        var zoomed = T.ZoomAt(T.Center, 2.0, 800, 600);
        Assert.Equal(T.MapUnitsPerPixel / 2, zoomed.MapUnitsPerPixel, 9);
    }

    [Fact]
    public void Zoom_Clamps_Extreme_Scales()
    {
        var tiny = T.ZoomAt(T.Center, 1e30, 800, 600);
        Assert.Equal(ViewportTransform.MinMapUnitsPerPixel, tiny.MapUnitsPerPixel);

        var huge = T.ZoomAt(T.Center, 1e-30, 800, 600);
        Assert.Equal(ViewportTransform.MaxMapUnitsPerPixel, huge.MapUnitsPerPixel);
    }

    [Fact]
    public void PanPixels_Moves_Center_Opposite_To_Drag_Direction()
    {
        // Dragging right by 100 px should move the map content right,
        // i.e. the camera center moves LEFT in map space by 100 * MapUnitsPerPixel.
        var panned = T.PanPixels(new ViewportPoint(100, 0));
        Assert.Equal(T.Center.X - 100 * T.MapUnitsPerPixel, panned.Center.X, 9);
        Assert.Equal(T.Center.Y, panned.Center.Y, 9);

        // Dragging down by 50 px moves center UP in map space (map Y is up).
        var pannedDown = T.PanPixels(new ViewportPoint(0, 50));
        Assert.Equal(T.Center.Y + 50 * T.MapUnitsPerPixel, pannedDown.Center.Y, 9);
    }
}
