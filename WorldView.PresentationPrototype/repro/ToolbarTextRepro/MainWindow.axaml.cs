using System;
using Avalonia.Controls;

namespace ToolbarTextRepro;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();

        // Match the prototype: the first two buttons get their Content from code-behind.
        Load1KButton.Content = "Load 1K";
        Load10KButton.Content = "Load 10K";

        var mode = "off";
        var delay = false;
        var suite = false;
        foreach (var arg in Environment.GetCommandLineArgs())
        {
            const string prefix = "--mode=";
            if (arg.StartsWith(prefix, StringComparison.Ordinal))
                mode = arg[prefix.Length..];
            if (arg == "--delay")
                delay = true;
            if (arg == "--suite")
                suite = true;
        }

        // Match the prototype: run the startup shaping suite before the first render.
        if (suite)
            ShapingSuite.Run();

        var probe = new GlyphRunProbe { Mode = delay ? "off" : mode };
        ProbeHost.Children.Add(probe);

        if (delay)
        {
            // Simulate the prototype's frame order: toolbar text is rendered first, then the
            // hand-built GlyphRun drawing appears on a later frame.
            var timer = new Avalonia.Threading.DispatcherTimer
            {
                Interval = TimeSpan.FromMilliseconds(1500)
            };
            timer.Tick += (_, _) =>
            {
                timer.Stop();
                probe.Mode = mode;
            };
            timer.Start();
        }
    }
}
