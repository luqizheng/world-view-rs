using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using Avalonia.Threading;
using WorldView.PresentationPrototype.Diagnostics;

namespace WorldView.PresentationPrototype;

/// <summary>
/// Automated performance observation run (Handoff §8 TV-06): drives the real desktop app
/// through the 1K / 10K / 50K fixture phases with continuous invalidation (so frames are
/// actually rendered instead of relying on input events), then records the diagnostics
/// counters per phase to a text file. This is observation, not a benchmark claim.
/// </summary>
public static class AutoBenchmark
{
    private const string DefaultOutputPath = "tv01-bench-results.txt";

    public static async Task RunAsync(MainWindow window, string? outputPath = null)
    {
        outputPath ??= DefaultOutputPath;
        var d = RenderDiagnostics.Instance;

        // Continuous frame clock: invalidate at ~60 Hz so every phase renders real frames.
        var frameClock = new DispatcherTimer(TimeSpan.FromMilliseconds(16), DispatcherPriority.Render,
            (_, _) => window.InvalidateVisual());
        frameClock.Start();

        var lines = new System.Collections.Generic.List<string>
        {
            $"TV-01 auto benchmark - {DateTimeOffset.Now:O}",
            $"OS: {Environment.OSVersion.VersionString} | CPU cores: {Environment.ProcessorCount}",
        };

        async Task MeasureAsync(string phase, int features, int stableMs)
        {
            d.ResetPhaseCounters();
            var framesBefore = d.FrameCount; // 0 after reset
            var sw = Stopwatch.StartNew();
            window.RequestScene(features);

            // Wait until rendering settles: no new frames for stableMs after the scene build.
            var lastFrameCount = -1L;
            var quietFor = 0;
            while (sw.Elapsed < TimeSpan.FromSeconds(60))
            {
                await Task.Delay(250);
                var now = d.FrameCount;
                if (now == lastFrameCount)
                {
                    quietFor += 250;
                    if (quietFor >= stableMs && d.LastFeatureCount == features) break;
                }
                else
                {
                    quietFor = 0;
                    lastFrameCount = now;
                }
            }

            var phaseFrames = d.FrameCount - framesBefore;
            lines.Add(
                $"{phase}: features={d.LastFeatureCount} prims={d.LastDrawnPrimitiveCount} " +
                $"buildMs={d.LastSceneBuildMs:F1} snap=v{d.PublishedVersion} " +
                $"frames={phaseFrames} lastMs={d.LastFrameMs:F2} avgMs={d.AvgFrameMs:F2} worstMs={d.WorstFrameMs:F2} " +
                $"wsMb={d.WorkingSetBytes / 1024 / 1024} gcMb={d.ManagedMemoryBytes / 1024 / 1024} dpi={d.LastScaling:F2}");
        }

        // Warm-up so JIT and font caches don't pollute the 1K numbers.
        await MeasureAsync("warmup", 256, 1000);

        await MeasureAsync("1K", 1_000, 1500);
        await MeasureAsync("10K", 10_000, 1500);
        await MeasureAsync("50K", 50_000, 1500);

        // Interaction pulses: rapid zoom in/out while rendering (worst-case frame timing).
        var zoomSw = Stopwatch.StartNew();
        while (zoomSw.Elapsed < TimeSpan.FromSeconds(2))
        {
            window.ZoomAtCenter(1.4);
            await Task.Delay(30);
        }
        while (zoomSw.Elapsed < TimeSpan.FromSeconds(4))
        {
            window.ZoomAtCenter(0.7);
            await Task.Delay(30);
        }

        lines.Add(
            $"zoom-pulse: frames={d.FrameCount} lastMs={d.LastFrameMs:F2} avgMs={d.AvgFrameMs:F2} worstMs={d.WorstFrameMs:F2}");
        lines.Add($"violations={d.VersionConsistencyViolations} discardedPublishes={d.DiscardedVersionCount} shaped={d.ShapedTextCount}");
        lines.Add(d.FormatSummary());

        frameClock.Stop();

        var dir = Path.GetDirectoryName(Path.GetFullPath(outputPath));
        if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
        await File.WriteAllLinesAsync(outputPath, lines);

        lines.ForEach(Console.WriteLine);
    }
}
