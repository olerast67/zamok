using System.Security.Cryptography;

namespace Zamok.Client;

/// <summary>
/// Проверка, что ключ восстановления записан: человек вводит 4 слова по номерам
/// (DESIGN.md, раздел 2.1). Номера выбираются случайно и не повторяются.
/// </summary>
public sealed class RecoveryCheck
{
    public const int WordsToCheck = 4;

    private readonly string[] _words;

    /// <summary>Номера слов (с единицы) по возрастанию.</summary>
    public IReadOnlyList<int> Positions { get; }

    public RecoveryCheck(string recoveryKey, IReadOnlyList<int>? positions = null)
    {
        _words = Split(recoveryKey);
        if (_words.Length < WordsToCheck)
            throw new ArgumentException("recovery key is too short", nameof(recoveryKey));
        Positions = positions ?? Pick(_words.Length);
    }

    public static string[] Split(string key) =>
        key.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);

    /// <summary>
    /// Слова ключа из любого текста, который вставил человек: просто слова через пробел или
    /// весь файл ключа (заголовок, таблица «1. слово», подсказка). Если в тексте есть номера
    /// вида «12.», берётся слово после каждого номера, по порядку номеров.
    /// </summary>
    public static string Normalize(string text)
    {
        var tokens = Split(text);
        var numbered = new SortedDictionary<int, string>();
        for (var i = 0; i + 1 < tokens.Length; i++)
        {
            var t = tokens[i];
            if (t.Length is >= 2 and <= 3 && t[^1] == '.' && int.TryParse(t[..^1], out var n) && n > 0)
                numbered[n] = tokens[i + 1];
        }
        var words = numbered.Count > 0 ? numbered.Values.ToArray() : tokens;
        return string.Join(' ', words).ToLowerInvariant();
    }

    /// <summary>Таблица для файла и печати: строки по 4 слова с номерами.</summary>
    public static string FormatTable(string key)
    {
        var words = Split(key);
        var lines = words.Chunk(4).Select((row, r) =>
            string.Concat(row.Select((w, i) => $"{r * 4 + i + 1,2}. {w,-13}")).TrimEnd());
        return string.Join("\r\n", lines);
    }

    private static int[] Pick(int count)
    {
        var set = new SortedSet<int>();
        while (set.Count < WordsToCheck)
            set.Add(RandomNumberGenerator.GetInt32(count) + 1);
        return [.. set];
    }

    /// <summary>Верно ли введено слово номер <paramref name="position"/>.</summary>
    public bool IsCorrect(int position, string? input) =>
        input is not null &&
        string.Equals(input.Trim(), _words[position - 1], StringComparison.OrdinalIgnoreCase);

    public bool AllCorrect(IReadOnlyList<string?> inputs) =>
        inputs.Count == Positions.Count &&
        Positions.Select((p, i) => IsCorrect(p, inputs[i])).All(ok => ok);
}
