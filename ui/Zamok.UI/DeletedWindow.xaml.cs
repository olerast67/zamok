using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Вопрос после удаления папки в Проводнике (DESIGN.md, раздел 5.1). Окно запускает core
/// (<c>Zamok.exe --deleted &lt;путь&gt;</c>) и ждёт ответа по каналу. «Вернуть»: папка снова на
/// месте запертой. «Убрать из Zamok»: папка уходит из списка, зашифрованные данные остаются
/// в скрытой папке, рядом появляется памятка «КАК ОТКРЫТЬ». Окно закрыли без ответа: core
/// возвращает папку сам, как без вопроса.
/// </summary>
public sealed partial class DeletedWindow : Window
{
    private const int DialogWidth = 520;
    private readonly string _path;
    private readonly CoreClient _client = new();

    public DeletedWindow(string path)
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
        RemoveNote.Text = Strings.Format("Deleted_RemoveNote", name);
        Title = name;
        AutomationProperties.SetName(Busy, Strings.Get("A11y_Busy"));
        WindowHelper.HookKeys(Root, () => OnRestoreClick(RestoreButton, new RoutedEventArgs()), Close);
        WindowHelper.FitHeight(this, Root, DialogWidth);
        // Окно запускает фоновый core, а не человек: Windows не даёт ему стать активным, поэтому
        // оно поверх остальных окон, как системный вопрос, и проверка не ждёт активации
        if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter p) p.IsAlwaysOnTop = true;
        Root.Loaded += OnLoaded;
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        Root.Loaded -= OnLoaded;
        RestoreButton.Focus(FocusState.Programmatic);
        try
        {
            var folder = UnlockWindow.FindFolder((await _client.StatusAsync()).Folders, _path);
            // Ответ уже дали или папку убрали другим способом: спрашивать не о чем
            if (folder is not { Asking: true }) Close();
        }
        catch (CoreNotRunningException) { ShowFatal(Strings.Error("core_not_running")); }
        catch (CoreNotTrustedException) { ShowFatal(Strings.Error("core_not_trusted")); }
    }

    private void OnRestoreClick(object sender, RoutedEventArgs e) => _ = AnswerAsync(remove: false);

    private void OnRemoveClick(object sender, RoutedEventArgs e) => _ = AnswerAsync(remove: true);

    private async Task AnswerAsync(bool remove)
    {
        if (!RestoreButton.IsEnabled) return;
        SetBusy(true);
        try
        {
            var r = await _client.AnswerDeletedAsync(_path, remove);
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
        RestoreButton.IsEnabled = !busy;
        RemoveButton.IsEnabled = !busy;
    }

    private void ShowError(string text)
    {
        WindowHelper.Announce(ErrorText, text);
        WindowHelper.FitHeight(this, Root, DialogWidth);
    }

    private void ShowFatal(string text)
    {
        ShowError(text);
        RestoreButton.IsEnabled = false;
        RemoveButton.IsEnabled = false;
    }
}
