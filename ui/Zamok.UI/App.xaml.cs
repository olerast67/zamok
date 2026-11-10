using Microsoft.UI.Xaml;

namespace Zamok.UI;

/// <summary>
/// Zamok.exe запускается из Проводника с одним действием:
/// <c>--unlock &lt;путь&gt;</c> (значок «Открыть Имя.zamok» или пункт меню) или
/// <c>--encrypt &lt;путь&gt;</c> («Зашифровать паролем»). Без аргументов ничего не показывает.
/// <c>--deleted &lt;путь&gt;</c> запускает сам core: папку удалили в Проводнике, вернуть или убрать.
/// </summary>
public partial class App : Application
{
    private Window? _window;

    public App()
    {
        // Язык окон тот же, что у core, меню Проводника и установщика: язык интерфейса Windows,
        // русский или английский. Без этого Windows на третьем языке показывала бы окна
        // по-русски (язык сборки по умолчанию), а трей по-английски
        Microsoft.Windows.Globalization.ApplicationLanguages.PrimaryLanguageOverride = Lang.Code;
        InitializeComponent();
        // Последняя страховка: неожиданная ошибка показывается, а окно остаётся
        UnhandledException += (_, e) =>
        {
            e.Handled = true;
            WindowHelper.Error(Strings.Get("App_Unexpected"));
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var (action, path) = ParseArgs(Environment.GetCommandLineArgs().Skip(1).ToArray());
        if (action == "--make-icons")
        {
            // При установке: собрать значки из значков этой Windows, без окна
            _ = MakeIconsAndExitAsync(path!);
            return;
        }
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
            "--deleted" => new DeletedWindow(path!),
            "--settings" => new SettingsWindow(),
            _ => null,
        };
        if (_window is null)
        {
            Exit();
            return;
        }
        _window.Activate();
    }

    private async Task MakeIconsAndExitAsync(string dir)
    {
        try { await IconMaker.MakeAllAsync(dir); } catch { Environment.ExitCode = 1; }
        Exit();
    }

    private async Task LockAndExitAsync(string path)
    {
        try
        {
            var client = new Zamok.Client.CoreClient();
            var folder = UnlockWindow.FindFolder((await client.StatusAsync()).Folders, path);
            if (folder is { OpenFiles: > 0 } &&
                !WindowHelper.Confirm(Strings.Format("Lock_OpenFiles", folder.Name, folder.OpenFiles)))
            {
                Exit();
                return;
            }
            var r = await client.LockAsync(path);
            if (!r.Ok) WindowHelper.Error(Strings.Error(r.Error));
        }
        catch (Zamok.Client.CoreNotRunningException) { WindowHelper.Error(Strings.Error("core_not_running")); }
        catch (Zamok.Client.CoreNotTrustedException) { WindowHelper.Error(Strings.Error("core_not_trusted")); }
        Exit();
    }

    internal static (string? Action, string? Path) ParseArgs(string[] args)
    {
        if (args.Length == 1 && args[0] == "--settings") return (args[0], null);
        if (args.Length == 2 && args[0] is "--unlock" or "--encrypt" or "--lock" or "--change-password" or "--show-key" or "--decrypt" or "--make-icons" or "--deleted" && args[1].Length > 0)
            return (args[0], args[1]);
        return (null, null);
    }
}
