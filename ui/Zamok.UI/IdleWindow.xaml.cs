using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Вопрос перед запиранием по бездействию, когда в папке открыты файлы (DESIGN.md, раздел 2.3).
/// Окно запускает core (<c>Zamok.exe --idle-lock &lt;путь&gt;</c>) и ждёт ответа по каналу.
/// «Отложить на 10 минут»: core не запирает папку по бездействию 10 минут, потом спросит снова.
/// «Запереть сейчас»: как «Запереть» в меню, несохранённое в открытых файлах пропадёт. Окно
/// закрыли без ответа: core запрёт папку, когда файлы закроют. Если папку тем временем заперли
/// (Win+L, сон) или с ней снова работают, core закрывает окно сам.
/// </summary>
public sealed partial class IdleWindow : Window
{
    private const int DialogWidth = 460;
    private readonly string _path;
    private readonly CoreClient _client = new();

    public IdleWindow(string path)
    {
        _path = path.TrimEnd('\\');
        InitializeComponent();
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, DialogWidth, 300);
        var icon = WindowHelper.FolderIcon(40);
        if (icon is not null) FolderIcon.Source = icon;
        else FallbackIcon.Visibility = Visibility.Visible;
        var name = Path.GetFileName(_path);
        FolderName.Text = name;
        FolderPath.Text = Path.GetDirectoryName(_path) ?? "";
        ToolTipService.SetToolTip(FolderPath, FolderPath.Text);
        Title = name;
        AutomationProperties.SetName(Busy, Strings.Get("A11y_Busy"));
        WindowHelper.HookKeys(Root, () => OnPostponeClick(PostponeButton, new RoutedEventArgs()), Close);
        WindowHelper.FitHeight(this, Root, DialogWidth);
        // Окно запускает фоновый core, а не человек: Windows не даёт ему стать активным, поэтому
        // оно поверх остальных окон, как системный вопрос
        if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter p) p.IsAlwaysOnTop = true;
        Root.Loaded += OnLoaded;
    }

    /// <summary>Текст о том, где открыты файлы: имена программ или хотя бы число файлов.</summary>
    internal static string OpenInText(FolderInfo f) => f.OpenIn is { Length: > 0 }
        ? Strings.Format("Idle_OpenIn", string.Join(", ", f.OpenIn))
        : Strings.Format("Idle_OpenCount", f.OpenFiles);

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        Root.Loaded -= OnLoaded;
        PostponeButton.Focus(FocusState.Programmatic);
        try
        {
            var folder = UnlockWindow.FindFolder((await _client.StatusAsync()).Folders, _path);
            // Ответ уже дали, папку заперли или файлы закрыли: спрашивать не о чем
            if (folder is not { IdleAsking: true })
            {
                Close();
                return;
            }
            Message.Text = Strings.Format("Idle_Message", Math.Max(1, folder.IdleLockSecs / 60));
            OpenIn.Text = OpenInText(folder);
            WindowHelper.FitHeight(this, Root, DialogWidth);
        }
        catch (CoreNotRunningException) { ShowFatal(Strings.Error("core_not_running")); }
        catch (CoreNotTrustedException) { ShowFatal(Strings.Error("core_not_trusted")); }
    }

    private void OnPostponeClick(object sender, RoutedEventArgs e) => _ = AnswerAsync(lockNow: false);

    private void OnLockClick(object sender, RoutedEventArgs e) => _ = AnswerAsync(lockNow: true);

    private async Task AnswerAsync(bool lockNow)
    {
        if (!PostponeButton.IsEnabled) return;
        SetBusy(true);
        try
        {
            var r = await _client.AnswerIdleAsync(_path, lockNow);
            if (r.Ok)
            {
                Close();
                return;
            }
            ShowError(Strings.Error(r.Error));
        }
        catch (CoreNotRunningException) { ShowError(Strings.Error("core_not_running")); }
        catch (CoreNotTrustedException)
        {
            ShowFatal(Strings.Error("core_not_trusted"));
            return;
        }
        SetBusy(false);
    }

    private void SetBusy(bool busy)
    {
        Busy.IsActive = busy;
        Busy.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        PostponeButton.IsEnabled = !busy;
        LockButton.IsEnabled = !busy;
    }

    private void ShowError(string text)
    {
        WindowHelper.Announce(ErrorText, text);
        WindowHelper.FitHeight(this, Root, DialogWidth);
    }

    private void ShowFatal(string text)
    {
        ShowError(text);
        PostponeButton.IsEnabled = false;
        LockButton.IsEnabled = false;
    }
}
