namespace Zamok.Client;

/// <summary>Итог одной строки «Проверки следов»: хорошо, стоит что-то сделать, не удалось узнать.</summary>
public enum TraceState { Good, Warning, Unknown }

/// <summary>
/// Решения «Проверки следов» (DESIGN.md, раздел 6) по уже прочитанным значениям. Само чтение
/// (свойство оболочки, реестр, область индексирования) в окне, здесь только логика, чтобы её
/// проверяли обычные тесты. Ничего не меняет: только смотрит.
/// </summary>
public static class TraceChecks
{
    /// <summary>Шифрование системного диска.</summary>
    public enum Disk { On, Encrypting, Paused, Waiting, Off, Unknown }

    /// <summary>
    /// Значение свойства оболочки <c>System.Volume.BitLockerProtection</c> (читается без прав
    /// администратора, в отличие от <c>manage-bde</c> и WMI). Свойство недокументированное, значения
    /// по наблюдениям: 1 включено, 2 выключено, 3 шифруется, 4 расшифровывается, 5 приостановлено,
    /// 6 включено и том заперт, 8 зашифрован, но ждёт включения защиты (шифрование устройства до
    /// входа с учётной записью Microsoft: ключ лежит на диске открыто). Остальное: не знаем.
    /// </summary>
    public static Disk DiskFromBitLocker(int? value) => value switch
    {
        1 or 6 => Disk.On,
        3 => Disk.Encrypting,
        5 => Disk.Paused,
        8 => Disk.Waiting,
        2 or 4 => Disk.Off,
        _ => Disk.Unknown,
    };

    public static TraceState StateOf(Disk d) => d switch
    {
        Disk.On => TraceState.Good,
        Disk.Unknown => TraceState.Unknown,
        _ => TraceState.Warning,
    };

    /// <summary>Гибернация: файл гибернации хранит содержимое памяти, в том числе открытые файлы.</summary>
    public enum Hiber { Off, OnSafe, On, Unknown }

    /// <summary>
    /// <paramref name="filepresent"/>: <c>HiberFilePresent</c> из <c>GetPwrCapabilities</c>;
    /// <paramref name="enabled"/>: <c>HibernateEnabled</c> из реестра, если он есть, иначе
    /// <c>HibernateEnabledDefault</c>. Файл гибернации всегда на системном диске.
    /// </summary>
    public static Hiber Hibernation(bool? filepresent, int? enabled, Disk disk)
    {
        bool? on = filepresent switch
        {
            true => true,
            false => enabled is null ? false : enabled != 0,
            null => enabled is null ? null : enabled != 0,
        };
        return on switch
        {
            null => Hiber.Unknown,
            false => Hiber.Off,
            true => disk == Disk.On ? Hiber.OnSafe : Hiber.On,
        };
    }

    public static TraceState StateOf(Hiber h) => h switch
    {
        Hiber.Off or Hiber.OnSafe => TraceState.Good,
        Hiber.On => TraceState.Warning,
        _ => TraceState.Unknown,
    };

    /// <summary>Файл подкачки.</summary>
    public enum PageFile { Encrypted, OnEncryptedDisk, Plain, Unknown }

    /// <summary>
    /// <paramref name="encryptValue"/>: <c>NtfsEncryptPagingFile</c> из
    /// <c>HKLM\SYSTEM\CurrentControlSet\Control\FileSystem</c> (там хранит его
    /// <c>fsutil behavior set EncryptPagingFile</c>), null, если прочитать не удалось.
    /// <paramref name="pagingFiles"/>: <c>PagingFiles</c> из «Memory Management»; файл подкачки
    /// под защитой шифрования диска, только если все файлы на системном диске (<c>?:</c> значит
    /// «размер по выбору системы», файл на системном диске).
    /// </summary>
    public static PageFile PageFileState(int? encryptValue, string[]? pagingFiles, string systemDrive, Disk disk)
    {
        if (encryptValue is > 0) return PageFile.Encrypted;
        if (disk == Disk.On && pagingFiles is not null && OnlyOnDrive(pagingFiles, systemDrive))
            return PageFile.OnEncryptedDisk;
        return encryptValue is null ? PageFile.Unknown : PageFile.Plain;
    }

    internal static bool OnlyOnDrive(string[] pagingFiles, string systemDrive)
    {
        var letter = systemDrive.TrimEnd('\\', ':');
        foreach (var line in pagingFiles)
        {
            var f = line.Trim();
            if (f.Length == 0) continue;
            if (f.StartsWith("?:", StringComparison.Ordinal)) continue;
            if (!f.StartsWith(letter + ":", StringComparison.OrdinalIgnoreCase)) return false;
        }
        return true;
    }

    public static TraceState StateOf(PageFile p) => p switch
    {
        PageFile.Encrypted or PageFile.OnEncryptedDisk => TraceState.Good,
        PageFile.Plain => TraceState.Warning,
        _ => TraceState.Unknown,
    };

    /// <summary>Индекс поиска Windows.</summary>
    public enum Index { NoFolders, SearchOff, Outside, Inside, Unknown }

    /// <summary>
    /// <paramref name="inside"/>: папки Zamok, которые сейчас входят в область индексирования;
    /// null, если служба поиска не ответила (выключена или её нет).
    /// </summary>
    public static Index IndexState(int folders, bool coreAnswered, IReadOnlyCollection<string>? inside, bool searchRunning)
    {
        if (!coreAnswered) return Index.Unknown;
        if (folders == 0) return Index.NoFolders;
        if (!searchRunning) return Index.SearchOff;
        if (inside is null) return Index.Unknown;
        return inside.Count == 0 ? Index.Outside : Index.Inside;
    }

    public static TraceState StateOf(Index i) => i switch
    {
        Index.NoFolders or Index.SearchOff or Index.Outside => TraceState.Good,
        Index.Inside => TraceState.Warning,
        _ => TraceState.Unknown,
    };

    /// <summary>Кэш миниатюр (исследование 05).</summary>
    public enum Thumbs { IconsOnly, Cached }

    /// <summary><c>IconsOnly</c> из <c>HKCU\…\Explorer\Advanced</c>: «Всегда отображать значки, а не эскизы».</summary>
    public static Thumbs ThumbsState(int? iconsOnly) => iconsOnly is > 0 ? Thumbs.IconsOnly : Thumbs.Cached;

    public static TraceState StateOf(Thumbs t) => t == Thumbs.IconsOnly ? TraceState.Good : TraceState.Warning;

    /// <summary>Адрес папки для области индексирования: <c>file:///C:\a\b\</c>, как в <c>search.rs</c>.</summary>
    public static string FolderUrl(string path) => "file:///" + path.TrimEnd('\\') + "\\";
}
