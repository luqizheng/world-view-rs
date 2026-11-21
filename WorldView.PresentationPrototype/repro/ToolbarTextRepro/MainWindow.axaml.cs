using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace ToolbarTextRepro;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        AvaloniaXamlLoader.Load(this);
    }
}
