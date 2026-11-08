using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Windows.System;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Пункты подменю Zamok: <c>--change-password</c> (старый пароль, новый дважды) и
/// <c>--show-key</c> (пароль, затем ключ восстановления той же сеткой, что в мастере).
/// </summary>
public sealed partial class ManageWindow : Window
{
    public enum Mode { ChangePassword, ShowKey }

    private readonly Mode _mode;
    private readonly string _path;
    private readonly string _name;
    private readonly CoreClient _client = new();
    private string? _key;
    private bool _done;

    public ManageWindow(Mode mode, string path)
    {
        _mode = mode;
        _path = path.TrimEnd('\\');
        _name = Path.GetFileName(_path);
        InitializeComponent();
        var change = mode == Mode.ChangePassword;
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, 440, change ? 400 : 290);
        var icon = WindowHelper.FolderIcon(32);
        if (icon is not null) FolderIcon.Source = icon;
        else FallbackIcon.Visibility = Visibility.Visible;
        Heading.Text = _name;
        StepText.Text = Strings.Get(change ? "Manage_ChangeStep" : "Manage_KeyStep");
        NewPassword1.Visibility = NewPassword2.Visibility = StrengthBar.Visibility =
            StrengthText.Visibility = change ? Visibility.Visible : Visibility.Collapsed;
        OkButton.Content = Strings.Get(change ? "Manage_Change" : "Manage_Show");
        UpdateOk();
        Activated += (_, _) => { if (!_done) Password.Focus(FocusState.Programmatic); };
        Password.PasswordChanged += (_, _) => UpdateOk();
    }

    private void ShowError(string text)
    {
        ErrorText.Text = text;
        ErrorText.Visibility = string.IsNullOrEmpty(text) ? Visibility.Collapsed : Visibility.Visible;
    }

    private void OnNewChanged(object sender, RoutedEventArgs e)
    {
        var score = PasswordStrength.Score(NewPassword1.Password);
        StrengthBar.Value = score;
        StrengthText.Text = Strings.Get($"Encrypt_Strength{score}");
        if (NewPassword2.Password.Length > 0 && NewPassword1.Password != NewPassword2.Password)
            StrengthText.Text += " " + Strings.Get("Encrypt_Mismatch");
        UpdateOk();
    }

    private void UpdateOk()
    {
        if (_done) { OkButton.IsEnabled = true; return; }
        OkButton.IsEnabled = !Busy.IsActive && Password.Password.Length > 0 &&
            (_mode == Mode.ShowKey ||
             (NewPassword1.Password.Length >= PasswordStrength.MinLength &&
              NewPassword1.Password == NewPassword2.Password));
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && _mode == Mode.ShowKey && OkButton.IsEnabled)
        {
            e.Handled = true;
            OnOkClick(sender, e);
        }
    }

    private async void OnOkClick(object sender, RoutedEventArgs e)
    {
        if (_done)
        {
            Close();
            return;
        }
        Busy.IsActive = true;
        UpdateOk();
        ShowError("");
        try
        {
            if (_mode == Mode.ChangePassword)
            {
                var r = await _client.ChangePasswordAsync(_path, Password.Password, NewPassword1.Password);
                if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
                Finish(Strings.Get("Manage_Changed"), 440, 240);
                PageInput.Visibility = Visibility.Collapsed;
                PageDone.Visibility = Visibility.Visible;
            }
            else
            {
                var r = await _client.ShowRecoveryKeyAsync(_path, Password.Password);
                if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
                _key = r.RecoveryKey;
                KeyView.Fill(KeyGrid, KeyWords, _key!);
                Finish(null, 520, 470);
                PageInput.Visibility = Visibility.Collapsed;
                PageKey.Visibility = Visibility.Visible;
            }
        }
        catch (CoreNotRunningException)
        {
            ShowError(Strings.Error("core_not_running"));
        }
        catch (CoreNotTrustedException)
        {
            ShowError(Strings.Error("core_not_trusted"));
        }
        finally
        {
            Password.Password = NewPassword1.Password = NewPassword2.Password = "";
            Busy.IsActive = false;
            UpdateOk();
        }
    }

    private void Finish(string? text, int width, int height)
    {
        _done = true;
        if (text is not null) DoneText.Text = text;
        OkButton.Content = Strings.Get("Common_Close");
        CancelButton.Visibility = Visibility.Collapsed;
        WindowHelper.Resize(this, width, height);
    }

    private async void OnSaveKeyClick(object sender, RoutedEventArgs e)
    {
        if (await KeyView.SaveAsync(this, _name, _key!)) KeySaved.Text = Strings.Get("Encrypt_KeySaved");
    }

    private void OnCopyKeyClick(object sender, RoutedEventArgs e)
    {
        KeyView.Copy(_key!);
        KeySaved.Text = Strings.Get("Encrypt_KeyCopied");
    }

    private void OnCancelClick(object sender, RoutedEventArgs e) => Close();
}
