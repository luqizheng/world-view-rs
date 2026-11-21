using System;
using System.Linq;
using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;

namespace WorldView.PresentationPrototype;

public partial class App : Application
{
    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);

        // --theme-fluent: expert-review follow-up (TV-01 known issue #1). Swaps SimpleTheme
        // for FluentTheme to test whether the toolbar text corruption is theme-specific.
        if (Environment.GetCommandLineArgs().Contains("--theme-fluent"))
        {
            Styles.Clear();
            Styles.Add(new Avalonia.Themes.Fluent.FluentTheme());
        }
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.ShutdownMode = Avalonia.Controls.ShutdownMode.OnMainWindowClose;
            desktop.MainWindow = new MainWindow();
        }

        base.OnFrameworkInitializationCompleted();
    }
}
