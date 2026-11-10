using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Окно настроек (DESIGN.md, раздел 2.5): список зашифрованных папок, свойства шифрования
/// выбранной, время автозапирания, «Запирать при блокировке экрана», «О программе».
/// Состояние папок обновляется раз в 2 секунды, пока окно открыто.
/// </summary>
public sealed partial class SettingsWindow : Window
{
    /// <summary>Варианты автозапирания в минутах; 0 = не запирать по времени.</summary>
    private static readonly ulong[] IdleMinutes = [5, 15, 30, 60, 120, 0];

    private readonly CoreClient _client = new();
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromSeconds(2) };
    private FolderInfo[] _folders = [];
    private string? _selectedId;
    private bool _updating;
    private bool _helloAvailable;
    private bool _coreDownShown;
    private string? _fitKey;
    /// <summary>Хранилища из предложения про Defender, которое сейчас на экране.</summary>
    private string[] _defenderOffer = [];

    public SettingsWindow()
    {
        InitializeComponent();
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, 680, 540);
        Title = Strings.Get("Settings_Title/Text");
        WindowHelper.HookKeys(Root, null, Close);
        // Кнопка со значком: имя для экранного диктора из её текста
        AutomationProperties.SetName(DefenderExclude, Strings.Get("Defender_ExcludeText/Text"));
        DefenderExclude.AccessKey = Strings.Get("Access_Exclude");
        var icon = WindowHelper.FolderIcon(32);
        if (icon is not null) FolderIcon.Source = icon;
        foreach (var m in IdleMinutes)
            IdleBox.Items.Add(m == 0 ? Strings.Get("Settings_IdleNever") : Strings.Format("Settings_IdleMinutes", m));
        _timer.Tick += async (_, _) => await RefreshAsync();
        Activated += async (_, _) =>
        {
            if (_timer.IsEnabled) return;
            _helloAvailable = await Hello.IsAvailableAsync();
            _timer.Start();
            await RefreshAsync();
        };
        Closed += (_, _) => _timer.Stop();
    }

    private async Task RefreshAsync()
    {
        try
        {
            await RefreshCoreAsync();
        }
        catch (Exception)
        {
            ShowError(Strings.Error("internal"));
        }
    }

    private async Task RefreshCoreAsync()
    {
        Response status;
        try
        {
            status = await _client.StatusAsync();
        }
        catch (Exception e) when (e is CoreNotRunningException or CoreNotTrustedException)
        {
            ShowError(Strings.Error(e is CoreNotRunningException ? "core_not_running" : "core_not_trusted"));
            _coreDownShown = true;
            return;
        }
        if (_coreDownShown)
        {
            _coreDownShown = false;
            ShowError("");
        }
        _updating = true;
        try
        {
            ScreenLockSwitch.IsOn = status.Settings?.LockOnScreenLock ?? true;
            _defenderOffer = status.Settings?.DefenderOffer ?? [];
            DefenderCard.Visibility = _defenderOffer.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            _folders = status.Folders ?? [];
            // Пересобрать список, только если состав или состояния поменялись
            var shown = string.Join("|", _folders.Select(f => f.Id + f.State));
            if ((string?)FolderList.Tag != shown)
            {
                FolderList.Tag = shown;
                FolderList.Items.Clear();
                foreach (var f in _folders)
                {
                    var row = new StackPanel { Padding = new Thickness(0, 4, 0, 4) };
                    row.Children.Add(new TextBlock { Text = f.Name, TextTrimming = TextTrimming.CharacterEllipsis });
                    // Цвета темы лежат в словарях тем, поиск по ключу их не находит: вторичный текст
                    // делаем мельче и прозрачнее
                    row.Children.Add(new TextBlock { Text = StateText(f), FontSize = 12, Opacity = 0.7 });
                    var item = new ListViewItem { Content = row, Tag = f.Id };
                    AutomationProperties.SetName(item, $"{f.Name}: {StateText(f)}");
                    FolderList.Items.Add(item);
                }
                var index = Array.FindIndex(_folders, f => f.Id == _selectedId);
                FolderList.SelectedIndex = index >= 0 ? index : (_folders.Length > 0 ? 0 : -1);
            }
            Empty.Visibility = _folders.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
            ShowDetails();
        }
        finally
        {
            _updating = false;
        }
    }

    private static string StateText(FolderInfo f) => Strings.Get(f.State switch
    {
        "unlocked" => "Settings_StateOpen",
        "locked" => "Settings_StateLocked",
        _ => "Settings_StateUnavailable",
    });

    private FolderInfo? Selected => _folders.FirstOrDefault(f => f.Id == _selectedId);

    private void OnFolderSelected(object sender, SelectionChangedEventArgs e)
    {
        if (FolderList.SelectedItem is ListViewItem item) _selectedId = (string)item.Tag;
        var was = _updating;
        _updating = true;
        ShowDetails();
        _updating = was;
    }

    private void ShowDetails()
    {
        var f = Selected;
        Details.Visibility = f is null ? Visibility.Collapsed : Visibility.Visible;
        if (f is null) return;
        DetailName.Text = f.Name;
        // Недоступна: сразу видно почему, а не только слово «недоступна»
        DetailState.Text = f.State == "unavailable" && f.Error is not null
            ? $"{StateText(f)}: {Strings.Error(f.Error)}"
            : StateText(f);
        ToggleButton.Content = Strings.Get(f.IsUnlocked ? "Settings_Lock" : "Settings_Open");
        ToggleButton.AccessKey = Strings.Get(f.IsUnlocked ? "Access_Lock" : "Access_Open");
        ToggleButton.IsEnabled = f.State != "unavailable";
        PropPath.Text = Strings.Format("Settings_PropPath", f.Path);
        PropVault.Text = Strings.Format("Settings_PropVault", VaultPath(f.Path));
        HelloPanel.Visibility = _helloAvailable ? Visibility.Visible : Visibility.Collapsed;
        HelloSwitch.IsOn = f.Hello;
        var minutes = f.IdleLockSecs / 60;
        var idx = Array.IndexOf(IdleMinutes, minutes);
        IdleBox.SelectedIndex = idx >= 0 ? idx : Array.IndexOf(IdleMinutes, 30UL);
        FitToContent();
    }

    /// <summary>Подогнать высоту, только если состав окна поменялся: иначе таймер двигал бы окно.</summary>
    private void FitToContent()
    {
        var key = $"{_folders.Length}|{_selectedId}|{Selected?.State}|{Selected?.Error}|{HelloPanel.Visibility}|{ErrorText.Text}|{DefenderCard.Visibility}|{StatusText.Text}";
        if (key == _fitKey) return;
        _fitKey = key;
        WindowHelper.FitHeight(this, Root, 680);
    }

    /// <summary>Хранилище лежит рядом с папкой: <c>.Имя.zamok</c>.</summary>
    internal static string VaultPath(string path)
    {
        var parent = Path.GetDirectoryName(path) ?? "";
        return Path.Combine(parent, "." + Path.GetFileName(path) + ".zamok");
    }

    private void ShowError(string text)
    {
        WindowHelper.Announce(ErrorText, text);
        FitToContent();
    }

    private async void OnToggleClick(object sender, RoutedEventArgs e)
    {
        var f = Selected;
        if (f is null) return;
        ShowError("");
        if (f.IsUnlocked)
        {
            if (f.OpenFiles > 0 && !WindowHelper.Confirm(
                    Strings.LockOpenFiles(f), WindowHelper.Handle(this))) return;
            try
            {
                var r = await _client.LockAsync(f.Path);
                if (!r.Ok) ShowError(Strings.Error(r.Error));
            }
            catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException) { ShowCoreError(ex); }
            await RefreshAsync();
        }
        else
        {
            new UnlockWindow(f.Path).Activate();
        }
    }

    private void OnShowClick(object sender, RoutedEventArgs e)
    {
        if (Selected is { } f)
            System.Diagnostics.Process.Start("explorer.exe", $"/select,\"{f.Path}\"");
    }

    private async void OnIdleChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_updating || Selected is not { } f || IdleBox.SelectedIndex < 0) return;
        var secs = IdleMinutes[IdleBox.SelectedIndex] * 60;
        if (secs == f.IdleLockSecs) return;
        try
        {
            var r = await _client.SetIdleLockAsync(f.Path, secs);
            if (!r.Ok) ShowError(Strings.Error(r.Error));
        }
        catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException) { ShowCoreError(ex); }
        await RefreshAsync();
    }

    private void ShowCoreError(Exception e) =>
        ShowError(Strings.Error(e is CoreNotRunningException ? "core_not_running" : "core_not_trusted"));

    private async void OnScreenLockToggled(object sender, RoutedEventArgs e)
    {
        if (_updating) return;
        try { await _client.SetLockOnScreenLockAsync(ScreenLockSwitch.IsOn); }
        catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException) { ShowCoreError(ex); }
    }

    private async void OnHelloToggled(object sender, RoutedEventArgs e)
    {
        if (_updating || Selected is not { } f || HelloSwitch.IsOn == f.Hello) return;
        ShowError("");
        try
        {
            if (!HelloSwitch.IsOn)
            {
                await _client.HelloDisableAsync(f.Path);
            }
            else
            {
                // Включить: пароль папки, затем подтверждение Windows Hello
                var box = new PasswordBox { Header = Strings.Get("Settings_HelloPassword") };
                var dialog = new ContentDialog
                {
                    XamlRoot = Root.XamlRoot,
                    Title = Strings.Get("Settings_HelloTitle"),
                    Content = box,
                    PrimaryButtonText = Strings.Get("Settings_HelloEnable"),
                    CloseButtonText = Strings.Get("Common_Cancel/Content"),
                    DefaultButton = ContentDialogButton.Primary,
                };
                var ok = await dialog.ShowAsync() == ContentDialogResult.Primary && box.Password.Length > 0;
                var password = box.Password;
                box.Password = "";
                string? key = ok ? await Hello.WrapKeyAsync(f.Id, create: true) : null;
                if (key is null)
                {
                    if (ok) ShowError(Strings.Error("hello_failed"));
                }
                else
                {
                    var r = await _client.HelloEnableAsync(f.Path, password, key);
                    if (!r.Ok) ShowError(Strings.Error(r.Error));
                }
            }
        }
        catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException) { ShowCoreError(ex); }
        await RefreshAsync();
        HelloSwitch.IsOn = Selected?.Hello ?? false;
    }

    private void ShowStatus(string text)
    {
        WindowHelper.Announce(StatusText, text);
        FitToContent();
    }

    /// <summary>Исключить хранилища из проверки Defender: одна команда с запросом прав (UAC).</summary>
    private async void OnDefenderExcludeClick(object sender, RoutedEventArgs e)
    {
        var vaults = _defenderOffer;
        if (vaults.Length == 0) return;
        ShowError("");
        ShowStatus("");
        SetDefenderBusy(true);
        try
        {
            var psi = new System.Diagnostics.ProcessStartInfo("powershell.exe", DefenderCommand.ElevatedArguments(vaults))
            {
                UseShellExecute = true,
                Verb = "runas",
                WindowStyle = System.Diagnostics.ProcessWindowStyle.Hidden,
            };
            int code;
            using (var p = System.Diagnostics.Process.Start(psi))
            {
                if (p is null) throw new InvalidOperationException();
                await p.WaitForExitAsync();
                code = p.ExitCode;
            }
            if (code != 0)
            {
                ShowError(Strings.Get("Defender_Failed"));
                return;
            }
            await DefenderDoneAsync(vaults);
            ShowStatus(Strings.Get("Defender_Done"));
        }
        catch (System.ComponentModel.Win32Exception ex) when (ex.NativeErrorCode == 1223)
        {
            // Отказ в окне UAC: предложение остаётся
        }
        catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException or ArgumentException)
        {
            ShowError(Strings.Get("Defender_Failed"));
        }
        finally
        {
            SetDefenderBusy(false);
        }
    }

    /// <summary>Скопировать команду: человек выполнит её сам в PowerShell от имени администратора.</summary>
    private async void OnDefenderCopyClick(object sender, RoutedEventArgs e)
    {
        var vaults = _defenderOffer;
        if (vaults.Length == 0) return;
        string command;
        try { command = DefenderCommand.Text(vaults); }
        catch (ArgumentException)
        {
            ShowError(Strings.Get("Defender_Failed"));
            return;
        }
        var data = new Windows.ApplicationModel.DataTransfer.DataPackage();
        data.SetText(command);
        if (!await WindowHelper.ClipboardAsync(() =>
            {
                Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(data);
                Windows.ApplicationModel.DataTransfer.Clipboard.Flush();
            }))
        {
            ShowError(Strings.Get("Common_ClipboardBusy"));
            return;
        }
        await DefenderDoneAsync(vaults);
        ShowStatus(Strings.Get("Defender_Copied"));
    }

    private async void OnDefenderNoClick(object sender, RoutedEventArgs e)
    {
        if (_defenderOffer.Length > 0) await DefenderDoneAsync(_defenderOffer);
    }

    private async Task DefenderDoneAsync(string[] vaults)
    {
        try
        {
            var r = await _client.DefenderOfferDoneAsync(vaults);
            if (!r.Ok) ShowError(Strings.Error(r.Error));
        }
        catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException) { ShowCoreError(ex); }
        await RefreshAsync();
    }

    private void SetDefenderBusy(bool busy)
    {
        DefenderExclude.IsEnabled = !busy;
        DefenderCopy.IsEnabled = !busy;
        DefenderNo.IsEnabled = !busy;
    }

    /// <summary>«Проверка следов» (DESIGN.md, раздел 6) в своём окне.</summary>
    private void OnTracesClick(object sender, RoutedEventArgs e) => new TracesWindow().Activate();

    private void OnCloseClick(object sender, RoutedEventArgs e) => Close();
}
