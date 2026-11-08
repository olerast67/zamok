using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage.Pickers;
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

    public EncryptWindow(string path)
    {
        _path = path.TrimEnd('\\');
        _name = Path.GetFileName(_path);
        InitializeComponent();
        WindowHelper.SetupDialog(this, 580, 620);
        Heading.Text = Strings.Format("Encrypt_Heading", _name);
        ShowPage(Page.Password);
        Activated += (_, _) => { if (_page == Page.Password) Password1.Focus(FocusState.Programmatic); };
        Closed += (_, _) => _poll?.Stop();
    }

    private void ShowPage(Page page)
    {
        _page = page;
        PagePassword.Visibility = Vis(page == Page.Password);
        PageKey.Visibility = Vis(page == Page.Key);
        PageVerify.Visibility = Vis(page is Page.Verify);
        PageProgress.Visibility = Vis(page is Page.Progress or Page.Done);
        BackButton.Visibility = Vis(page == Page.Verify);
        LockNowButton.Visibility = Vis(page == Page.Done && _lastJob?.IsDone == true);
        ErrorText.Text = "";
        NextButton.Content = Strings.Get(page switch
        {
            Page.Password => "Encrypt_Start",
            Page.Verify => "Encrypt_Check",
            Page.Done => "Common_Close",
            _ => "Common_Next",
        });
        NextButton.Visibility = Vis(page != Page.Progress);
        CancelButton.Visibility = Vis(page is Page.Password or Page.Progress);
        UpdateNext();
    }

    private static Visibility Vis(bool v) => v ? Visibility.Visible : Visibility.Collapsed;

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
        if (NoKeyWarning is null || MakeKey is null) return;
        NoKeyWarning.Visibility = Vis(MakeKey.IsChecked != true);
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
                ErrorText.Text = Strings.Error(r.Error) +
                    (r.Examples is { Length: > 0 } ex ? "\n" + string.Join("\n", ex.Take(8)) : "");
                return;
            }
            _jobId = r.Job!.Id;
            Password1.Password = Password2.Password = "";
            StartPolling();
            if (MakeKey.IsChecked == true)
            {
                _recoveryKey = r.RecoveryKey;
                ShowKey(_recoveryKey!);
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
            ErrorText.Text = Strings.Error("core_not_running");
        }
        catch (CoreNotTrustedException)
        {
            ErrorText.Text = Strings.Error("core_not_trusted");
        }
        finally
        {
            Busy.IsActive = false;
            Password1.IsEnabled = Password2.IsEnabled = _page == Page.Password;
            UpdateNext();
        }
    }

    /// <summary>Слова с номерами по четыре в строке: так их удобно переписывать и сверять.</summary>
    private void ShowKey(string key)
    {
        var words = RecoveryCheck.Split(key);
        KeyGrid.Children.Clear();
        KeyGrid.RowDefinitions.Clear();
        for (var i = 0; i < words.Length; i++)
        {
            if (i % 4 == 0) KeyGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var cell = new TextBlock
            {
                Text = $"{i + 1,2}. {words[i]}",
                FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Cascadia Mono, Consolas"),
                IsTextSelectionEnabled = true,
            };
            Grid.SetRow(cell, i / 4);
            Grid.SetColumn(cell, i % 4);
            KeyGrid.Children.Add(cell);
        }
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(KeyWords,
            string.Join(" ", words.Select((w, i) => $"{i + 1}. {w}")));
    }

    // 2. Ключ

    private async void OnSaveKeyClick(object sender, RoutedEventArgs e)
    {
        var picker = new FileSavePicker { SuggestedFileName = Strings.Format("Encrypt_KeyFileName", _name) };
        picker.FileTypeChoices.Add(Strings.Get("Encrypt_KeyFileType"), [".txt"]);
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WindowHelper.Handle(this));
        var file = await picker.PickSaveFileAsync();
        if (file is null) return;
        var text = Strings.Format("Encrypt_KeyFileText", _name,
            DateTime.Now.ToString("yyyy-MM-dd HH:mm"), RecoveryCheck.FormatTable(_recoveryKey!)) + "\r\n";
        await Windows.Storage.FileIO.WriteTextAsync(file, text);
        KeySaved.Text = Strings.Get("Encrypt_KeySaved");
    }

    private void OnCopyKeyClick(object sender, RoutedEventArgs e)
    {
        var data = new DataPackage();
        data.SetText(_recoveryKey);
        Clipboard.SetContent(data);
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
            ErrorText.Text = Strings.Get("Encrypt_WordsWrong");
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
                if (r.Job is { } job) RefreshProgress(job);
            }
            catch (Exception ex) when (ex is CoreNotRunningException or CoreNotTrustedException)
            {
                _poll?.Stop();
                if (_page == Page.Progress) ErrorText.Text = Strings.Error("core_not_running");
            }
        };
        _poll.Start();
    }

    private JobInfo? _lastJob;

    private void RefreshProgress(JobInfo? job)
    {
        job ??= _lastJob;
        _lastJob = job;
        if (job is null) return;
        if (!job.IsRunning) _poll?.Stop();
        if (_page != Page.Progress) return; // человек ещё записывает ключ
        Progress.Value = job.Fraction;
        ProgressText.Text = Strings.Format("Encrypt_ProgressFiles", job.FilesDone, job.FilesTotal,
            job.BytesDone / 1048576.0, job.BytesTotal / 1048576.0);
        // Отменить можно только пока идёт копирование
        CancelButton.IsEnabled = job.IsRunning && job.Phase == "copying";
        if (job.IsRunning)
        {
            ProgressTitle.Text = Strings.Get(job.Phase == "copying" ? "Encrypt_Copying" : "Encrypt_Finishing");
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
            ErrorText.Text = Strings.Error(job.Error);
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

    private async void OnLockNowClick(object sender, RoutedEventArgs e)
    {
        LockNowButton.IsEnabled = false;
        try { await _client.LockAsync(_path); } catch { }
        Close();
    }

    private async void OnCancelClick(object sender, RoutedEventArgs e)
    {
        if (_page == Page.Progress && _jobId is not null && !_finished)
        {
            try { await _client.CancelJobAsync(_jobId); } catch { }
            return; // итог «отменено» придёт со следующим опросом
        }
        Close();
    }
}
