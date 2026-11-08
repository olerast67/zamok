using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Animation;
using Windows.System;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Окно пароля (DESIGN.md, раздел 2.2): имя папки крупно, поле пароля в фокусе, Enter открывает.
/// Неверный пароль: поле вздрагивает, «Пароль не подошёл». Запасной путь: ключ восстановления.
/// </summary>
public sealed partial class UnlockWindow : Window
{
    private readonly string _path;
    private readonly CoreClient _client = new();
    private bool _recoveryMode;

    public UnlockWindow(string path)
    {
        _path = path;
        InitializeComponent();
        WindowHelper.SetupDialog(this, 460, 400);
        FolderName.Text = Path.GetFileName(path.TrimEnd('\\'));
        Activated += OnFirstActivated;
    }

    private async void OnFirstActivated(object sender, WindowActivatedEventArgs e)
    {
        Activated -= OnFirstActivated;
        Password.Focus(FocusState.Programmatic);
        try
        {
            var status = await _client.StatusAsync();
            var folder = FindFolder(status.Folders, _path);
            if (folder is null)
            {
                ShowFatal(Strings.Error("not_found"));
                return;
            }
            FolderName.Text = folder.Name;
            if (folder.IsUnlocked) Close(); // уже открыта: окно не нужно
        }
        catch (CoreNotRunningException)
        {
            ShowFatal(Strings.Error("core_not_running"));
        }
        catch (CoreNotTrustedException)
        {
            ShowFatal(Strings.Error("core_not_trusted"));
        }
    }

    /// <summary>Папка, к которой относится путь (сама папка или значок внутри неё).</summary>
    internal static FolderInfo? FindFolder(FolderInfo[]? folders, string path)
    {
        var want = path.TrimEnd('\\');
        return folders?.FirstOrDefault(f =>
            want.Equals(f.Path, StringComparison.OrdinalIgnoreCase) ||
            want.StartsWith(f.Path.TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase));
    }

    private void ShowFatal(string text)
    {
        ErrorText.Text = text;
        Password.IsEnabled = false;
        RecoveryWords.IsEnabled = false;
        OpenButton.IsEnabled = false;
        ModeLink.IsEnabled = false;
    }

    private void OnInputKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter)
        {
            e.Handled = true;
            OnOpenClick(sender, e);
        }
        else if (e.Key == VirtualKey.Escape)
        {
            Close();
        }
    }

    private void OnModeClick(object sender, RoutedEventArgs e)
    {
        _recoveryMode = !_recoveryMode;
        Password.Visibility = _recoveryMode ? Visibility.Collapsed : Visibility.Visible;
        RecoveryWords.Visibility = _recoveryMode ? Visibility.Visible : Visibility.Collapsed;
        Prompt.Text = Strings.Get(_recoveryMode ? "Unlock_PromptRecovery/Text" : "Unlock_Prompt/Text");
        ModeLink.Content = Strings.Get(_recoveryMode ? "Unlock_UsePassword/Content" : "Unlock_UseRecovery/Content");
        ErrorText.Text = "";
        if (_recoveryMode) RecoveryWords.Focus(FocusState.Programmatic);
        else Password.Focus(FocusState.Programmatic);
    }

    private void OnCancelClick(object sender, RoutedEventArgs e) => Close();

    private async void OnOpenClick(object sender, RoutedEventArgs e)
    {
        var secret = _recoveryMode ? RecoveryCheck.Normalize(RecoveryWords.Text) : Password.Password;
        if (string.IsNullOrWhiteSpace(secret) || !OpenButton.IsEnabled) return;
        SetBusy(true);
        try
        {
            var r = _recoveryMode
                ? await _client.UnlockRecoveryAsync(_path, secret)
                : await _client.UnlockAsync(_path, secret);
            if (r.Ok)
            {
                Close();
                return;
            }
            ErrorText.Text = Strings.Error(r.Error);
            if (r.Error is "wrong_password" or "wrong_recovery_key") ShakeInput();
        }
        catch (CoreNotRunningException)
        {
            ErrorText.Text = Strings.Error("core_not_running");
        }
        catch (CoreNotTrustedException)
        {
            ShowFatal(Strings.Error("core_not_trusted"));
            return;
        }
        finally
        {
            Password.Password = "";
            SetBusy(false);
        }
        if (_recoveryMode) RecoveryWords.Focus(FocusState.Programmatic);
        else Password.Focus(FocusState.Programmatic);
    }

    private void SetBusy(bool busy)
    {
        Busy.IsActive = busy;
        OpenButton.IsEnabled = !busy;
        Password.IsEnabled = !busy;
        RecoveryWords.IsEnabled = !busy;
    }

    private void ShakeInput()
    {
        var anim = new DoubleAnimationUsingKeyFrames { Duration = TimeSpan.FromMilliseconds(400) };
        double[] xs = [0, -10, 10, -8, 8, -4, 4, 0];
        for (var i = 0; i < xs.Length; i++)
            anim.KeyFrames.Add(new LinearDoubleKeyFrame
            {
                Value = xs[i],
                KeyTime = TimeSpan.FromMilliseconds(i * 50),
            });
        Storyboard.SetTarget(anim, Shake);
        Storyboard.SetTargetProperty(anim, "X");
        var sb = new Storyboard();
        sb.Children.Add(anim);
        sb.Begin();
    }
}
