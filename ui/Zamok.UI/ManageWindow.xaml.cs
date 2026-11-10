using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
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
    private int _width = 520;

    public ManageWindow(Mode mode, string path)
    {
        _mode = mode;
        _path = path.TrimEnd('\\');
        _name = Path.GetFileName(_path);
        InitializeComponent();
        var change = mode == Mode.ChangePassword;
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, 520,
            change ? 400 : mode == Mode.Decrypt ? 330 : 290);
        var icon = WindowHelper.FolderIcon(32);
        if (icon is not null) FolderIcon.Source = icon;
        else FallbackIcon.Visibility = Visibility.Visible;
        Heading.Text = _name;
        Title = _name;
        AutomationProperties.SetName(StrengthBar, Strings.Get("A11y_Strength"));
        AutomationProperties.SetName(Progress, Strings.Get("A11y_Progress"));
        AutomationProperties.SetName(Busy, Strings.Get("A11y_Busy"));
        // Кнопки со значком: имя для экранного диктора из их текста
        AutomationProperties.SetName(SaveKeyButton, Strings.Get("Encrypt_SaveKeyText/Text"));
        AutomationProperties.SetName(CopyKeyButton, Strings.Get("Encrypt_CopyKeyText/Text"));
        SaveKeyButton.AccessKey = Strings.Get("Access_SaveKey");
        CopyKeyButton.AccessKey = Strings.Get("Access_CopyKey");
        WindowHelper.HookKeys(Root, OnEnter, OnEscape);
        Closed += (_, _) => KeyView.ClearIfStillOurs();
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
        WindowHelper.FitHeight(this, Root, _width);
        Activated += (_, _) => { if (!_done) Password.Focus(FocusState.Programmatic); };
        Password.PasswordChanged += (_, _) => UpdateOk();
    }

    private void ShowError(string text)
    {
        WindowHelper.Announce(ErrorText, text);
        WindowHelper.FitHeight(this, Root, _width);
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

    private void OnEnter()
    {
        if (!WindowHelper.CanPress(OkButton)) return;
        // Расшифровка необратима: Enter в поле пароля только переводит на кнопку, нажать её надо отдельно
        if (_mode == Mode.Decrypt && !_done) { OkButton.Focus(FocusState.Keyboard); return; }
        OnOkClick(OkButton, new RoutedEventArgs());
    }

    private void OnEscape()
    {
        // Пока идёт расшифровка, окно не закрываем случайным нажатием
        if (PageProgress.Visibility != Visibility.Visible) Close();
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
        var keepFields = false;
        try
        {
            if (_mode == Mode.ChangePassword)
            {
                var r = await _client.ChangePasswordAsync(_path, Password.Password, NewPassword1.Password);
                if (!r.Ok) { keepFields = true; ShowError(Strings.Error(r.Error)); return; }
                Finish(Strings.Get("Manage_Changed"), 520, 240);
                PageInput.Visibility = Visibility.Collapsed;
                PageDone.Visibility = Visibility.Visible;
            }
            else if (_mode == Mode.Decrypt)
            {
                var r = await _client.DecryptAsync(_path, Password.Password);
                if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
                if (r.Job is null) { ShowError(Strings.Error("internal")); return; }
                PageInput.Visibility = Visibility.Collapsed;
                PageProgress.Visibility = Visibility.Visible;
                OkButton.Visibility = CancelButton.Visibility = Visibility.Collapsed;
                WindowHelper.FitHeight(this, Root, _width);
                await WatchJob(r.Job.Id);
            }
            else
            {
                var r = await _client.ShowRecoveryKeyAsync(_path, Password.Password);
                if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
                _key = r.RecoveryKey;
                if (_key is null) { ShowError(Strings.Error("internal")); return; }
                KeyView.Fill(KeyGrid, KeyWords, _key);
                StepText.Text = Strings.Get("Encrypt_StepKey");
                Finish(null, 600, 470);
                PageInput.Visibility = Visibility.Collapsed;
                PageKey.Visibility = Visibility.Visible;
            }
        }
        catch (CoreNotRunningException)
        {
            keepFields = true;
            ShowError(Strings.Error("core_not_running"));
        }
        catch (CoreNotTrustedException)
        {
            ShowError(Strings.Error("core_not_trusted"));
        }
        finally
        {
            // Опечатка в одном поле не стирает остальные: старый пароль вводится заново
            Password.Password = "";
            if (!keepFields) NewPassword1.Password = NewPassword2.Password = "";
            Busy.IsActive = false;
            UpdateOk();
            if (!_done && PageInput.Visibility == Visibility.Visible) Password.Focus(FocusState.Programmatic);
        }
    }

    /// <summary>Следить за расшифровкой до конца. Закрытие окна работу не прерывает.</summary>
    private async Task WatchJob(string id)
    {
        while (true)
        {
            JobInfo? job;
            try { job = (await _client.JobStatusAsync(id)).Job; }
            catch (Exception e) when (e is CoreNotRunningException or CoreNotTrustedException)
            {
                LoseJob("core_not_running");
                return;
            }
            if (job is null) { LoseJob("internal"); return; }
            Progress.Value = job.Fraction;
            ProgressText.Text = Strings.Format("Encrypt_ProgressFiles", job.FilesDone, job.FilesTotal,
                job.BytesDone / 1048576.0, job.BytesTotal / 1048576.0);
            if (!job.IsRunning)
            {
                OkButton.Visibility = Visibility.Visible;
                PageProgress.Visibility = Visibility.Collapsed;
                if (job.IsDone)
                {
                    Finish(Strings.Format("Manage_Decrypted", _name), 520, 240);
                    PageDone.Visibility = Visibility.Visible;
                }
                else
                {
                    Finish(null, 520, 260);
                    ShowError(Strings.Error(job.Error));
                }
                return;
            }
            await Task.Delay(300);
        }
    }

    /// <summary>Связь с работой потеряна: окно можно закрыть, core доведёт работу после перезапуска.</summary>
    private void LoseJob(string error)
    {
        PageProgress.Visibility = Visibility.Collapsed;
        OkButton.Visibility = Visibility.Visible;
        Finish(null, 520, 260);
        ShowError(Strings.Error(error));
    }

    private void Finish(string? text, int width, int height)
    {
        _done = true;
        if (text is not null) DoneText.Text = text;
        OkButton.Content = Strings.Get("Common_Close");
        CancelButton.Visibility = Visibility.Collapsed;
        _width = width;
        WindowHelper.FitHeight(this, Root, width);
    }

    private async void OnSaveKeyClick(object sender, RoutedEventArgs e)
    {
        if (await KeyView.SaveAsync(this, _name, _key!)) WindowHelper.Announce(KeySaved, Strings.Get("Encrypt_KeySaved"));
    }

    private async void OnCopyKeyClick(object sender, RoutedEventArgs e)
    {
        var copied = await KeyView.CopyAsync(_key!);
        WindowHelper.Announce(KeySaved, Strings.Get(copied ? "Encrypt_KeyCopied" : "Common_ClipboardBusy"));
    }

    private void OnCancelClick(object sender, RoutedEventArgs e) => Close();
}
