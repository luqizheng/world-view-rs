using System;
using System.IO;
using Avalonia;

namespace WorldView.PresentationPrototype;

internal static class Program
{
    [STAThread]
    public static int Main(string[] args)
    {
        try
        {
            BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
            return 0;
        }
        catch (Exception ex)
        {
            // WinExe has no console; persist crashes for the validation record.
            try { File.WriteAllText("tv01-crash.log", ex.ToString()); } catch { /* best effort */ }
            Console.Error.WriteLine(ex);
            return 1;
        }
    }

    // Avalonia configuration: the single place where the text-shaping authority is configured.
    // UseHarfBuzz() installs the HarfBuzz-backed shaper into TextShaper.Current
    // (Handoff §6.7: one shaping authority; Skia never shapes).
    public static AppBuilder BuildAvaloniaApp()
    {
        var builder = AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .UseHarfBuzz()
            .LogToTrace();

        // --software: force CPU rendering to test whether the toolbar text corruption is
        // tied to the GPU compositor path (expert-review follow-up; see validation report).
        if (Environment.GetCommandLineArgs().Contains("--software"))
        {
            builder = builder.With(new Win32PlatformOptions
            {
                RenderingMode = new[] { Avalonia.Win32RenderingMode.Software }
            });
        }

        return builder;
    }
}
