using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage.Pickers;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>Ключ восстановления в окне: сетка 11 x 4 с номерами, сохранение в файл, копирование.</summary>
internal static partial class KeyView
{
    /// <summary>
    /// Заполнить сетку (4 столбца) словами с номерами. Каждое слово в своей ячейке: оно не
    /// отрывается от номера. Экранный диктор читает весь ключ из имени <paramref name="area"/>.
    /// </summary>
    public static void Fill(Grid grid, UIElement area, string key)
    {
        var words = RecoveryCheck.Split(key);
        grid.Children.Clear();
        grid.RowDefinitions.Clear();
        for (var i = 0; i < words.Length; i++)
        {
            if (i % 4 == 0) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var cell = new TextBlock
            {
                Text = $"{i + 1,2}. {words[i]}",
                FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Cascadia Mono, Consolas"),
                FontSize = 13,
                IsTextSelectionEnabled = true,
            };
            Grid.SetRow(cell, i / 4);
            Grid.SetColumn(cell, i % 4);
            grid.Children.Add(cell);
        }
        AutomationProperties.SetName(area, string.Join(" ", words.Select((w, i) => $"{i + 1}. {w}")));
    }

    /// <summary>Сохранить в текстовый файл таблицей с номерами и подсказкой. true, если сохранено.</summary>
    public static async Task<bool> SaveAsync(Window owner, string folderName, string key)
    {
        var picker = new FileSavePicker { SuggestedFileName = Strings.Format("Encrypt_KeyFileName", folderName) };
        picker.FileTypeChoices.Add(Strings.Get("Encrypt_KeyFileType"), [".txt"]);
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WindowHelper.Handle(owner));
        var file = await picker.PickSaveFileAsync();
        if (file is null) return false;
        var text = Strings.Format("Encrypt_KeyFileText", folderName,
            DateTime.Now.ToString("yyyy-MM-dd HH:mm"), RecoveryCheck.FormatTable(key)) + "\r\n";
        await Windows.Storage.FileIO.WriteTextAsync(file, text);
        return true;
    }

    private static DispatcherTimer? _clearTimer;
    private static uint _copiedSequence;

    /// <summary>
    /// В буфер, но мимо журнала Win+V и облачной синхронизации. Через минуту буфер очищается,
    /// если в нём всё ещё ключ: номер изменения буфера говорит об этом, не читая содержимое.
    /// </summary>
    public static async Task<bool> CopyAsync(string key)
    {
        var data = new DataPackage();
        data.SetText(key);
        var options = new ClipboardContentOptions { IsAllowedInHistory = false, IsRoamable = false };
        if (!await WindowHelper.ClipboardAsync(() => Clipboard.SetContentWithOptions(data, options))) return false;
        _copiedSequence = GetClipboardSequenceNumber();
        _clearTimer?.Stop();
        _clearTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(60) };
        _clearTimer.Tick += (_, _) => ClearIfStillOurs();
        _clearTimer.Start();
        return true;
    }

    /// <summary>Очистить буфер, если там ключ. Вызывается и при закрытии окна: таймер умрёт вместе с ним.</summary>
    public static void ClearIfStillOurs()
    {
        _clearTimer?.Stop();
        _clearTimer = null;
        if (_copiedSequence != 0 && GetClipboardSequenceNumber() == _copiedSequence) Clipboard.Clear();
        _copiedSequence = 0;
    }

    [LibraryImport("user32.dll")]
    private static partial uint GetClipboardSequenceNumber();
}
