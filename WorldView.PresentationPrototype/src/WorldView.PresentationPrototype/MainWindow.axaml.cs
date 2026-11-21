using System;
using System.Linq;
using System.IO;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using WorldView.PresentationPrototype.Diagnostics;
using WorldView.PresentationPrototype.Rendering;
using WorldView.PresentationPrototype.Scene;

namespace WorldView.PresentationPrototype;

/// <summary>
/// Desktop shell: toolbar + map viewport + diagnostics text box.
/// Owns the ScenePublisher, background SceneBuildService and a UI-side diagnostics poller.
/// All business logic lives in Rendering/Scene/Viewport/Text layers; this class is wiring only.
/// </summary>
public partial class MainWindow : Window
{
    private readonly ScenePublisher _publisher = new();
    private readonly SceneBuildService _buildService;
    private readonly MapViewport _mapViewport;
    private readonly DispatcherTimer _diagnosticsTimer;
    private int _lastSeed = 42;

    public MainWindow()
    {
        InitializeComponent();

        _buildService = new SceneBuildService(_publisher);
        _mapViewport = new MapViewport(_publisher);
        ViewportHost.Children.Add(_mapViewport);

        // Set the first two button contents from code-behind: the XAML Content string path
        // triggers a reproducible Avalonia 12.1.6 text-rendering corruption for these two
        // buttons only (see validation report, "toolbar text corruption" known issue).
        Load1KButton.Content = "Load 1K";
        Load10KButton.Content = "Load 10K";

        _diagnosticsTimer = new DispatcherTimer(TimeSpan.FromMilliseconds(500), DispatcherPriority.Background,
            (_, _) => DiagnosticsBox.Text = RenderDiagnostics.Instance.FormatSummary());
        _diagnosticsTimer.Start();

        Opened += async (_, _) =>
        {
            // Startup shaping suite runs after fonts are ready (evidence for TV-03).
            try
            {
                TextValidation.RunStartupShapingSuite();
                HintText.Text += " | Startup shaping suite: PASS (7 samples incl. CJK/RTL/combining).";
            }
            catch (Exception ex)
            {
                HintText.Text += " | Startup shaping suite FAILED: " + ex.Message;
            }

            _mapViewport.ResetView();

            // --autobench [path]: drive 1K/10K/50K phases automatically, write results, close.
            // --screenshot <path> [features]: publish a scene, render the window into an
            //   offscreen bitmap through the real Skia pipeline, save PNG, close.
            var args = Environment.GetCommandLineArgs();
            var benchIndex = Array.IndexOf(args, "--autobench");
            var shotIndex = Array.IndexOf(args, "--screenshot");
            if (benchIndex >= 0)
            {
                var benchPath = benchIndex + 1 < args.Length && !args[benchIndex + 1].StartsWith('-')
                    ? args[benchIndex + 1]
                    : null;
                _buildService.RequestBuild(64, _lastSeed); // initial snapshot for the shell
                await AutoBenchmark.RunAsync(this, benchPath);
                Close();
            }
            else if (shotIndex >= 0)
            {
                var shotPath = shotIndex + 1 < args.Length && !args[shotIndex + 1].StartsWith('-')
                    ? args[shotIndex + 1]
                    : "tv01-screenshot.png";
                var features = shotIndex + 2 < args.Length && int.TryParse(args[shotIndex + 2], out var f) ? f : 1000;
                await CaptureSceneAsync(shotPath, features);
                Close();
            }
            else
            {
                _buildService.RequestBuild(1000, _lastSeed);
            }
        };

        Closing += OnClosing;
    }

    /// <summary>Publish a scene, wait for the first frames, render the window offscreen via
    /// RenderTargetBitmap (real Skia path) and save the PNG. Independent of desktop capture
    /// state - usable over headless RDP sessions.</summary>
    private async Task CaptureSceneAsync(string path, int featureCount)
    {
        var tcs = new TaskCompletionSource<SceneSnapshot>(TaskCreationOptions.RunContinuationsAsynchronously);
        _publisher.Published += s => tcs.TrySetResult(s);
        _buildService.RequestBuild(featureCount, _lastSeed);

        var completed = await Task.WhenAny(tcs.Task, Task.Delay(30_000));
        if (completed != tcs.Task)
            throw new TimeoutException($"Scene with {featureCount} features was not published within 30 s.");

        // Give the compositor a couple of frames to present the new snapshot.
        await Task.Delay(1200);

        var scaling = LayoutHelper.GetRenderScaling(this);
        var pixelSize = new PixelSize(
            Math.Max(1, (int)(Bounds.Width * scaling)),
            Math.Max(1, (int)(Bounds.Height * scaling)));
        using var rtb = new RenderTargetBitmap(pixelSize, new Vector(scaling * 96, scaling * 96));
        rtb.Render(this);

        using var stream = File.Create(path);
        rtb.Save(stream);

        // Sanity note recorded beside the image: drawn version must match the published one.
        var d = RenderDiagnostics.Instance;
        var toolbarDump = string.Join("\n",
            new[] { Load1KButton, Load10KButton, Load50KButton, RebuildButton, CancelStressButton, ZoomInButton, ZoomOutButton, ResetButton }
            .Select(b => $"  {b.Name}: content='{b.Content}' bounds={b.Bounds.Width:F0}x{b.Bounds.Height:F0}@({b.Bounds.X:F0},{b.Bounds.Y:F0}) desired={b.DesiredSize.Width:F0}x{b.DesiredSize.Height:F0}"));
        var note = $"published v{d.PublishedVersion} drawn v{d.LastDrawnSnapshotVersion} prims {d.LastDrawnPrimitiveCount} " +
                   $"violations {d.VersionConsistencyViolations} backend {d.BackendInfo}\n" +
                   $"window bounds {Bounds.Width:F0}x{Bounds.Height:F0} client {ClientSize.Width}x{ClientSize.Height} scaling {scaling:F2}\n" +
                   $"toolbar:\n{toolbarDump}";
        await File.WriteAllTextAsync(Path.ChangeExtension(path, ".txt"), note);
    }

    private bool _shutdownCompleted;

    private async void OnClosing(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        if (_shutdownCompleted) return;

        // Stop background work before the window dies (Handoff §7 shutdown rule);
        // cancel the first close request, finish cleanup, then close for real.
        e.Cancel = true;
        _shutdownCompleted = true;
        _diagnosticsTimer.Stop();
        await _buildService.ShutdownAsync();
        _mapViewport.Renderer.Dispose();
        Closing -= OnClosing;
        Close();
    }

    private void OnLoad1K(object sender, RoutedEventArgs e) => Request(1_000);
    private void OnLoad10K(object sender, RoutedEventArgs e) => Request(10_000);
    private void OnLoad50K(object sender, RoutedEventArgs e) => Request(50_000);

    private void Request(int count)
    {
        _lastSeed++;
        _buildService.RequestBuild(count, _lastSeed);
    }

    /// <summary>Public entry for the auto benchmark: request a synthetic scene with N features.</summary>
    public void RequestScene(int featureCount) => Request(featureCount);

    /// <summary>Public entry for the auto benchmark: zoom at the viewport center.</summary>
    public void ZoomAtCenter(double factor) =>
        _mapViewport.ZoomAtPointer(new Avalonia.Point(ViewportHost.Bounds.Width / 2, ViewportHost.Bounds.Height / 2), factor);

    private void OnRebuild(object sender, RoutedEventArgs e) => Request(1_000);

    /// <summary>Fires 20 rapid rebuild requests; earlier builds must be discarded
    /// (version monotonicity), producing visible snapshot churn for TV-05.</summary>
    private void OnCancelStress(object sender, RoutedEventArgs e)
    {
        for (var i = 0; i < 20; i++)
            Request(64);
    }

    private void OnZoomIn(object sender, RoutedEventArgs e) =>
        _mapViewport.ZoomAtPointer(new Avalonia.Point(ViewportHost.Bounds.Width / 2, ViewportHost.Bounds.Height / 2), 1.25);

    private void OnZoomOut(object sender, RoutedEventArgs e) =>
        _mapViewport.ZoomAtPointer(new Avalonia.Point(ViewportHost.Bounds.Width / 2, ViewportHost.Bounds.Height / 2), 0.8);

    private void OnReset(object sender, RoutedEventArgs e) => _mapViewport.ResetView();
}
