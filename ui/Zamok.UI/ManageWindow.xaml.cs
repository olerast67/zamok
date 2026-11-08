using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Windows.System;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Пункты подменю Zamok: <c>--change-password</c> (старый пароль, новый дважды) и
/// <c>--show-key</c> (пароль, затем ключ восстановления той же сеткой, что в мастере),
/// <c>--decrypt</c> («Расшифровать навсегда»: пароль, ход работы, итог).
/// </summary>
public sealed partial class ManageWindow : Window
{
    public enum Mode { ChangePassword, ShowKey, Decrypt }

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
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, 440,
            change ? 400 : mode == Mode.Decrypt ? 330 : 290);
        var icon = WindowHelper.FolderIcon(32);
        if (icon is not null) FolderIcon.Source = icon;
        else FallbackIcon.Visibility = Visibility.Visible;
        Heading.Text = _name;
        StepText.Text = Strings.Get(mode switch
        {
            Mode.ChangePassword => "Manage_ChangeStep",
            Mode.Decrypt => "Manage_DecryptStep",
            _ => "Manage_KeyStep",
        });
        DecryptWarning.Visibility = mode == Mode.Decrypt ? Visibility.Visible : Visibility.Collapsed;
        NewPassword1.Visibility = NewPassword2.Visibility = StrengthBar.Visibility =
            StrengthText.Visibility = change ? Visibility.Visible : Visibility.Collapsed;
        OkButton.Content = Strings.Get(mode switch
        {
            Mode.ChangePassword => "Manage_Change",
            Mode.Decrypt => "Manage_Decrypt",
            _ => "Manage_Show",
        });
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
            (_mode != Mode.ChangePassword ||
             (NewPassword1.Password.Length >= PasswordStrength.MinLength &&
              NewPassword1.Password == NewPassword2.Password));
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && _mode != Mode.ChangePassword && OkButton.IsEnabled)
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
            else if (_mode == Mode.Decrypt)
            {
                var r = await _client.DecryptAsync(_path, Password.Password);
                if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
                PageInput.Visibility = Visibility.Collapsed;
                PageProgress.Visibility = Visibility.Visible;
                OkButton.Visibility = CancelButton.Visibility = Visibility.Collapsed;
                await WatchJob(r.Job!.Id);
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

    /// <summary>Следить за расшифровкой до конца. Закрытие окна работу не прерывает.</summary>
    private async Task WatchJob(string id)
    {
        while (true)
        {
            JobInfo job;
            try { job = (await _client.JobStatusAsync(id)).Job!; }
            catch (CoreNotRunningException) { ShowError(Strings.Error("core_not_running")); return; }
            Progress.Value = job.Fraction;
            ProgressText.Text = Strings.Format("Encrypt_ProgressFiles", job.FilesDone, job.FilesTotal,
                job.BytesDone / 1048576.0, job.BytesTotal / 1048576.0);
            if (!job.IsRunning)
            {
                OkButton.Visibility = Visibility.Visible;
                PageProgress.Visibility = Visibility.Collapsed;
                if (job.IsDone)
                {
                    Finish(Strings.Format("Manage_Decrypted", _name), 440, 240);
                    PageDone.Visibility = Visibility.Visible;
                }
                else
                {
                    Finish(null, 440, 260);
                    ShowError(Strings.Error(job.Error));
                }
                return;
            }
            await Task.Delay(300);
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
