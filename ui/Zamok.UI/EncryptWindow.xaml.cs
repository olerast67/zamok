using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// Мастер шифрования (DESIGN.md, раздел 2.1): пароль дважды с полосой надёжности,
/// ключ восстановления, проверка 4 слов, прогресс. Шифрование в core начинается сразу после
/// пароля и идёт, пока человек записывает ключ; закрытие окна работу не прерывает.
/// </summary>
public sealed partial class EncryptWindow : Window
{
    private enum Page { Password, Key, Verify, Progress, Done }

    private readonly string _path;
    private readonly string _name;
    private readonly CoreClient _client = new();
    private Page _page = Page.Password;
    private string? _recoveryKey;
    private string? _jobId;
    private RecoveryCheck? _check;
    private DispatcherTimer? _poll;
    private bool _finished;
    private bool _skipArmed;
    private bool _cancelling;
    private bool _coreLost;

    public EncryptWindow(string path)
    {
        _path = path.TrimEnd('\\');
        _name = Path.GetFileName(_path);
        InitializeComponent();
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, 440, 420);
        var icon = WindowHelper.FolderIcon(32);
        if (icon is not null) FolderIcon.Source = icon;
        else FallbackIcon.Visibility = Visibility.Visible;
        Heading.Text = Strings.Format("Encrypt_Heading", _name);
        Title = Heading.Text;
        AutomationProperties.SetName(StrengthBar, Strings.Get("A11y_Strength"));
        AutomationProperties.SetName(Progress, Strings.Get("A11y_Progress"));
        AutomationProperties.SetName(Busy, Strings.Get("A11y_Busy"));
        WindowHelper.HookKeys(Root, OnEnter, OnEscape);
        ShowPage(Page.Password);
        Activated += (_, _) => { if (_page == Page.Password) Password1.Focus(FocusState.Programmatic); };
        Closed += (_, _) => { _poll?.Stop(); KeyView.ClearIfStillOurs(); };
    }

    private void ShowPage(Page page)
    {
        _page = page;
        PagePassword.Visibility = Vis(page == Page.Password);
        PageKey.Visibility = Vis(page == Page.Key);
        PageVerify.Visibility = Vis(page is Page.Verify);
        PageProgress.Visibility = Vis(page is Page.Progress or Page.Done);
        BackButton.Visibility = Vis(page == Page.Verify);
        // Передумал хранить ключ: «Без ключа» на шагах с ключом, со вторым подтверждением
        _skipArmed = false;
        SkipKeyButton.Content = Strings.Get("Encrypt_SkipKey");
        SkipKeyButton.Visibility = Vis(page is Page.Key or Page.Verify);
        LockNowButton.Visibility = Vis(page == Page.Done && _lastJob?.IsDone == true);
        DoneIcon.Visibility = Vis(page == Page.Done && _lastJob?.IsDone == true);
        ShowError("");
        StepText.Text = Strings.Get(page switch
        {
            Page.Password => "Encrypt_StepPassword",
            Page.Key => "Encrypt_StepKey",
            Page.Verify => "Encrypt_StepVerify",
            Page.Progress => "Encrypt_StepProgress",
            _ => "Encrypt_StepDone",
        });
        // Окно ровно под шаг: ширина своя, высота по содержимому
        Fit();
        NextButton.Content = Strings.Get(page switch
        {
            Page.Password => "Encrypt_Start",
            Page.Verify => "Encrypt_Check",
            Page.Done => "Common_Close",
            _ => "Common_Next",
        });
        NextButton.Visibility = Vis(page != Page.Progress);
        CancelButton.Visibility = Vis(page != Page.Done);
        UpdateCancel();
        UpdateNext();
        if (_coreLost && page == Page.Progress) ShowError(Strings.Error("core_not_running"));
    }

    private static Visibility Vis(bool v) => v ? Visibility.Visible : Visibility.Collapsed;

    private void ShowError(string text)
    {
        ErrorText.Text = text;
        ErrorText.Visibility = Vis(!string.IsNullOrEmpty(text));
        Fit();
    }

    /// <summary>Отмена возможна до подмены папки; после неё ключ уже нужен, отменять нечего.</summary>
    private void UpdateCancel()
    {
        if (_page == Page.Password || _jobId is null) { CancelButton.IsEnabled = true; return; }
        CancelButton.IsEnabled = !_cancelling && !_finished && !_coreLost &&
            _lastJob is null or { IsRunning: true, Phase: "copying" };
    }

    private void OnEnter()
    {
        if (WindowHelper.CanPress(NextButton)) OnNextClick(NextButton, new RoutedEventArgs());
    }

    private void OnEscape()
    {
        // На шагах с ключом и в ходе работы Esc ничего не делает: ключ больше не покажут
        if (_page is Page.Password or Page.Done) Close();
    }

    private void Fit() => WindowHelper.FitHeight(this, Root, _page == Page.Key ? 520 : 460);

    // 1. Пароль

    private void OnPasswordChanged(object sender, RoutedEventArgs e)
    {
        var score = PasswordStrength.Score(Password1.Password);
        StrengthBar.Value = score;
        StrengthText.Text = Strings.Get($"Encrypt_Strength{score}");
        if (Password2.Password.Length > 0 && Password1.Password != Password2.Password)
            StrengthText.Text += " " + Strings.Get("Encrypt_Mismatch");
        UpdateNext();
    }

    private void OnMakeKeyChanged(object sender, RoutedEventArgs e)
    {
        // Checked срабатывает ещё при загрузке разметки, когда предупреждения пока нет
        if (NoKeyWarning is null || MakeKey is null || Root is null) return;
        NoKeyWarning.Visibility = Vis(MakeKey.IsChecked != true);
        if (_page == Page.Password) Fit();
    }

    private void UpdateNext()
    {
        NextButton.IsEnabled = _page switch
        {
            Page.Password => Password1.Password.Length >= PasswordStrength.MinLength &&
                             Password1.Password == Password2.Password && !Busy.IsActive,
            _ => true,
        };
    }

    private async Task StartEncryptionAsync()
    {
        Busy.IsActive = true;
        NextButton.IsEnabled = false;
        Password1.IsEnabled = Password2.IsEnabled = false;
        try
        {
            var r = await _client.EncryptAsync(_path, Password1.Password);
            if (!r.Ok)
            {
                ShowError(Strings.Error(r.Error) +
                    (r.Examples is { Length: > 0 } ex ? "\n" + string.Join("\n", ex.Take(8)) : ""));
                return;
            }
            if (r.Job is null) { ShowError(Strings.Error("internal")); return; }
            _jobId = r.Job.Id;
            Password1.Password = Password2.Password = "";
            StartPolling();
            if (MakeKey.IsChecked == true)
            {
                _recoveryKey = r.RecoveryKey;
                if (_recoveryKey is null) { ShowError(Strings.Error("internal")); return; }
                KeyView.Fill(KeyGrid, KeyWords, _recoveryKey);
                ShowPage(Page.Key);
            }
            else
            {
                // Человек выбрал вариант без ключа: слова нигде не показываются и не хранятся
                ShowPage(Page.Progress);
                RefreshProgress(null);
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
            Busy.IsActive = false;
            Password1.IsEnabled = Password2.IsEnabled = _page == Page.Password;
            UpdateNext();
        }
    }

    // 2. Ключ

    private async void OnSaveKeyClick(object sender, RoutedEventArgs e)
    {
        if (await KeyView.SaveAsync(this, _name, _recoveryKey!))
            KeySaved.Text = Strings.Get("Encrypt_KeySaved");
    }

    private void OnCopyKeyClick(object sender, RoutedEventArgs e)
    {
        KeyView.Copy(_recoveryKey!);
        KeySaved.Text = Strings.Get("Encrypt_KeyCopied");
    }

    // 3. Проверка

    private void PrepareVerify()
    {
        _check = new RecoveryCheck(_recoveryKey!);
        TextBox[] boxes = [Word1, Word2, Word3, Word4];
        for (var i = 0; i < boxes.Length; i++)
        {
            boxes[i].Header = Strings.Format("Encrypt_WordN", _check.Positions[i]);
            boxes[i].Text = "";
        }
        ShowPage(Page.Verify);
        Word1.Focus(FocusState.Programmatic);
    }

    private void Verify()
    {
        if (_check!.AllCorrect([Word1.Text, Word2.Text, Word3.Text, Word4.Text]))
        {
            ShowPage(Page.Progress);
            RefreshProgress(null);
        }
        else
        {
            ShowError(Strings.Get("Encrypt_WordsWrong"));
        }
    }

    // 4. Прогресс

    private void StartPolling()
    {
        _poll = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(300) };
        _poll.Tick += async (_, _) =>
        {
            try
            {
                var r = await _client.JobStatusAsync(_jobId!);
                if (r.Job is { } job) { RefreshProgress(job); return; }
                LosePolling();
            }
            catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException)
            {
                LosePolling();
            }
        };
        _poll.Start();
    }

    /// <summary>Связь с работой потеряна. На шаге с ключом покажем, когда человек дойдёт до хода работы.</summary>
    private void LosePolling()
    {
        _poll?.Stop();
        _coreLost = true;
        UpdateCancel();
        if (_page == Page.Progress) ShowError(Strings.Error("core_not_running"));
    }

    private JobInfo? _lastJob;

    private void RefreshProgress(JobInfo? job)
    {
        job ??= _lastJob;
        _lastJob = job;
        if (job is null) return;
        if (!job.IsRunning) _poll?.Stop();
        UpdateCancel();
        if (_page != Page.Progress) return; // человек ещё записывает ключ
        Progress.Value = job.Fraction;
        ProgressText.Text = Strings.Format("Encrypt_ProgressFiles", job.FilesDone, job.FilesTotal,
            job.BytesDone / 1048576.0, job.BytesTotal / 1048576.0);
        if (job.IsRunning)
        {
            ProgressTitle.Text = Strings.Get(_cancelling ? "Encrypt_Cancelling"
                : job.Phase == "copying" ? "Encrypt_Copying" : "Encrypt_Finishing");
        }
        else if (job.IsDone)
        {
            _finished = true;
            ProgressTitle.Text = Strings.Format("Encrypt_Done", _name);
            Progress.Value = 1;
            ShowPage(Page.Done);
        }
        else
        {
            _finished = true;
            ProgressTitle.Text = Strings.Get("Encrypt_Failed");
            ShowError(Strings.Error(job.Error));
            NextButton.Content = Strings.Get("Common_Close");
            NextButton.Visibility = Visibility.Visible;
            CancelButton.Visibility = Visibility.Collapsed;
            _page = Page.Done;
        }
    }

    // Кнопки

    private async void OnNextClick(object sender, RoutedEventArgs e)
    {
        switch (_page)
        {
            case Page.Password: await StartEncryptionAsync(); break;
            case Page.Key: PrepareVerify(); break;
            case Page.Verify: Verify(); break;
            case Page.Done: Close(); break;
        }
    }

    private void OnBackClick(object sender, RoutedEventArgs e) => ShowPage(Page.Key);

    private void OnSkipKeyClick(object sender, RoutedEventArgs e)
    {
        if (!_skipArmed)
        {
            _skipArmed = true;
            SkipKeyButton.Content = Strings.Get("Encrypt_SkipKeySure");
            ShowError(Strings.Get("Encrypt_NoKeyWarning/Text"));
            return;
        }
        // Ключ больше нигде не показывается и не хранится
        _recoveryKey = null;
        ShowPage(Page.Progress);
        RefreshProgress(null);
    }

    private async void OnLockNowClick(object sender, RoutedEventArgs e)
    {
        LockNowButton.IsEnabled = false;
        try
        {
            var r = await _client.LockAsync(_path);
            if (!r.Ok) { ShowError(Strings.Error(r.Error)); return; }
        }
        catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException)
        {
            ShowError(Strings.Error(ex is CoreNotRunningException ? "core_not_running" : "core_not_trusted"));
            return;
        }
        finally { LockNowButton.IsEnabled = true; }
        Close();
    }

    private async void OnCancelClick(object sender, RoutedEventArgs e)
    {
        if (_page != Page.Password && _jobId is not null && !_finished)
        {
            CancelButton.IsEnabled = false;
            _cancelling = true;
            try
            {
                var r = await _client.CancelJobAsync(_jobId);
                if (!r.Ok)
                {
                    // Поздно: шифрование дойдёт до конца, поэтому шаг с ключом не покидаем
                    _cancelling = false;
                    ShowError(Strings.Get("Encrypt_TooLate"));
                    UpdateCancel();
                    return;
                }
            }
            catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException)
            {
                _cancelling = false;
                ShowError(Strings.Error(ex is CoreNotRunningException ? "core_not_running" : "core_not_trusted"));
                UpdateCancel();
                return;
            }
            // Итог «отменено» придёт со следующим опросом
            if (_page != Page.Progress) ShowPage(Page.Progress);
            ProgressTitle.Text = Strings.Get("Encrypt_Cancelling");
            RefreshProgress(null);
            return;
        }
        Close();
    }
}
