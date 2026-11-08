using Microsoft.UI.Xaml;

namespace Zamok.UI;

/// <summary>
/// Zamok.exe запускается из Проводника с одним действием:
/// <c>--unlock &lt;путь&gt;</c> (значок «Открыть Имя.zamok» или пункт меню) или
/// <c>--encrypt &lt;путь&gt;</c> («Зашифровать паролем»). Без аргументов ничего не показывает.
/// </summary>
public partial class App : Application
{
    private Window? _window;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var (action, path) = ParseArgs(Environment.GetCommandLineArgs().Skip(1).ToArray());
        if (action == "--lock")
        {
            // Пункт меню «Запереть»: без окна, запереть и выйти
            _ = LockAndExitAsync(path!);
            return;
        }
        _window = action switch
        {
            "--unlock" => new UnlockWindow(path!),
            "--encrypt" => new EncryptWindow(path!),
            "--change-password" => new ManageWindow(ManageWindow.Mode.ChangePassword, path!),
            "--show-key" => new ManageWindow(ManageWindow.Mode.ShowKey, path!),
            "--decrypt" => new ManageWindow(ManageWindow.Mode.Decrypt, path!),
            _ => null,
        };
        if (_window is null)
        {
            Exit();
            return;
        }
        _window.Activate();
    }

    private async Task LockAndExitAsync(string path)
    {
        try { await new Zamok.Client.CoreClient().LockAsync(path); } catch { }
        Exit();
    }

    internal static (string? Action, string? Path) ParseArgs(string[] args)
    {
        if (args.Length == 2 && args[0] is "--unlock" or "--encrypt" or "--lock" or "--change-password" or "--show-key" or "--decrypt" && args[1].Length > 0)
            return (args[0], args[1]);
        return (null, null);
    }
}
