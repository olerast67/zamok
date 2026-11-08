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
