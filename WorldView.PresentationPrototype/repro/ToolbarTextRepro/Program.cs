using System;
using System.IO;
using Avalonia;

namespace ToolbarTextRepro;

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
            // WinExe has no console; persist crashes for the experiment record.
            try { File.WriteAllText("repro-crash.log", ex.ToString()); } catch { /* best effort */ }
            Console.Error.WriteLine(ex);
            return 1;
        }
    }

    public static AppBuilder BuildAvaloniaApp()
        => AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .UseHarfBuzz()
            .LogToTrace();
}
