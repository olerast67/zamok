using System.Text;
using System.Text.Json;

namespace Zamok.Client;

/// <summary>
/// Команда, которая исключает скрытые хранилища из проверки Microsoft Defender
/// (DESIGN.md, раздел 7). Defender проверяет и открытые файлы через папку Zamok, и шифротекст
/// в хранилище; вторая проверка бесполезна. Менять настройки Defender может только
/// администратор, поэтому команда либо запускается с запросом прав (UAC), либо копируется.
/// </summary>
/// <remarks>
/// Пути в текст команды не попадают: PowerShell считает кавычками не только <c>'</c>, но и
/// типографские ‘ ’ ‚ ‛, а в двойных кавычках раскрывает <c>$(…)</c> и обратный апостроф.
/// Поэтому пути идут данными: массив JSON в UTF-8, закодированный base64 (в нём только
/// буквы, цифры, <c>+</c>, <c>/</c> и <c>=</c>), и разбираются уже внутри команды.
/// </remarks>
public static class DefenderCommand
{
    /// <summary>
    /// Строка для PowerShell. Путь с управляющими символами отклоняется:
    /// <see cref="ArgumentException"/>.
    /// </summary>
    public static string Text(IEnumerable<string> vaults)
    {
        var list = vaults.ToArray();
        if (list.Length == 0) throw new ArgumentException("no vaults", nameof(vaults));
        foreach (var v in list)
            if (string.IsNullOrEmpty(v) || v.Any(char.IsControl))
                throw new ArgumentException("a vault path has control characters", nameof(vaults));
        var json = JsonSerializer.Serialize(list);
        var data = Convert.ToBase64String(Encoding.UTF8.GetBytes(json));
        return "Add-MpPreference -ExclusionPath @(ConvertFrom-Json ([Text.Encoding]::UTF8.GetString(" +
               "[Convert]::FromBase64String('" + data + "'))))";
    }

    /// <summary>
    /// Аргументы powershell.exe для запуска с правами администратора: команда идёт
    /// в <c>-EncodedCommand</c> (UTF-16 в base64). Ошибка команды даёт ненулевой код выхода.
    /// </summary>
    public static string ElevatedArguments(IEnumerable<string> vaults)
    {
        var script = "$ErrorActionPreference = 'Stop'; " + Text(vaults);
        var encoded = Convert.ToBase64String(Encoding.Unicode.GetBytes(script));
        return "-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand " + encoded;
    }

    /// <summary>Пути, которые команда передаст Defender (для проверки: обратное к <see cref="Text"/>).</summary>
    public static string[] PathsIn(string command)
    {
        var start = command.IndexOf("FromBase64String('", StringComparison.Ordinal) + "FromBase64String('".Length;
        var end = command.IndexOf('\'', start);
        var json = Encoding.UTF8.GetString(Convert.FromBase64String(command[start..end]));
        return JsonSerializer.Deserialize<string[]>(json) ?? [];
    }
}
