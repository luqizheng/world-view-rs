using Avalonia;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;

[assembly: AvaloniaTestApplication(typeof(WorldView.PresentationPrototype.Tests.TestAppBuilder))]

namespace WorldView.PresentationPrototype.Tests;

/// <summary>
/// Headless test application: same App + HarfBuzz shaping configuration as the desktop app,
/// but with the headless platform backend so tests run without opening real windows.
/// UseHarfBuzz is REQUIRED here - it validates the same shaping authority the app uses.
/// </summary>
public static class TestAppBuilder
{
    public static AppBuilder BuildAvaloniaApp()
        => AppBuilder.Configure<global::WorldView.PresentationPrototype.App>()
            .UseHeadless(new AvaloniaHeadlessPlatformOptions())
            .UseHarfBuzz();
}
