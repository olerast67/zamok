using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace Zamok.Client;

/// <summary>
/// Чтение значений для «Проверки следов» (DESIGN.md, раздел 6). Только чтение, без прав
/// администратора: свойство оболочки, реестр, область индексирования поиска. Ни одна настройка
/// не меняется. Решения по прочитанному принимает <see cref="TraceChecks"/>.
/// </summary>
public static class TraceProbe
{
    public sealed record Result(
        string SystemDrive,
        TraceChecks.Disk Disk,
        TraceChecks.Hiber Hiber,
        TraceChecks.PageFile PageFile,
        TraceChecks.Index Index,
        string[] Indexed,
        TraceChecks.Thumbs Thumbs);

    /// <summary>Все проверки. <paramref name="folders"/>: пути папок Zamok, null, если core не ответил.</summary>
    public static Result Run(string[]? folders)
    {
        var drive = (Environment.GetEnvironmentVariable("SystemDrive") ?? "C:").TrimEnd('\\');
        var disk = TraceChecks.DiskFromBitLocker(BitLocker(drive + "\\"));
        var hiber = TraceChecks.Hibernation(HiberFilePresent(), HibernateEnabled(), disk);
        var page = TraceChecks.PageFileState(
            ReadInt(@"SYSTEM\CurrentControlSet\Control\FileSystem", "NtfsEncryptPagingFile"),
            ReadMulti(@"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management", "PagingFiles"),
            drive, disk);
        var found = folders is { Length: > 0 } ? InCrawlScope(folders) : null;
        var index = TraceChecks.IndexState(folders?.Length ?? 0, folders is not null, found, found is not null);
        var thumbs = TraceChecks.ThumbsState(
            ReadInt(@"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "IconsOnly", Registry.CurrentUser));
        return new Result(drive, disk, hiber, page, index, [.. found ?? []], thumbs);
    }

    /// <summary>
    /// Папки, которые сейчас входят в область индексирования (<c>IncludedInCrawlScope</c>, как
    /// <c>search::included</c> в core); null, если служба поиска выключена или её нет.
    /// </summary>
    internal static List<string>? InCrawlScope(IEnumerable<string> folders)
    {
        ISearchCrawlScopeManager scope;
        try
        {
            scope = ScopeManager();
        }
        catch (Exception e) when (e is COMException or InvalidCastException or UnauthorizedAccessException)
        {
            return null;
        }
        try
        {
            var found = new List<string>();
            foreach (var f in folders)
            {
                // Отказ по одной папке (например, путь недоступен) не роняет проверку
                if (scope.IncludedInCrawlScope(TraceChecks.FolderUrl(f), out var yes) == 0 && yes != 0)
                    found.Add(f);
            }
            return found;
        }
        finally
        {
            Marshal.ReleaseComObject(scope);
        }
    }

    /// <summary>
    /// <c>System.Volume.BitLockerProtection</c> корня диска. Так это значение показывает сам
    /// Проводник (значок замка на диске), права администратора не нужны.
    /// </summary>
    internal static int? BitLocker(string root)
    {
        try
        {
            if (PSGetPropertyKeyFromName("System.Volume.BitLockerProtection", out var key) != 0) return null;
            var iid = typeof(IShellItem2).GUID;
            if (SHCreateItemFromParsingName(root, IntPtr.Zero, ref iid, out var item) != 0 || item is null) return null;
            try
            {
                return item.GetInt32(ref key, out var v) == 0 ? v : null;
            }
            finally
            {
                Marshal.ReleaseComObject(item);
            }
        }
        catch (Exception e) when (e is COMException or InvalidCastException)
        {
            return null;
        }
    }

    /// <summary><c>HiberFilePresent</c> из <c>SYSTEM_POWER_CAPABILITIES</c> (смещение 8).</summary>
    internal static bool? HiberFilePresent()
    {
        var buf = new byte[128];
        try
        {
            return GetPwrCapabilities(buf) ? buf[8] != 0 : null;
        }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException)
        {
            return null;
        }
    }

    private static int? HibernateEnabled()
    {
        const string power = @"SYSTEM\CurrentControlSet\Control\Power";
        return ReadInt(power, "HibernateEnabled") ?? ReadInt(power, "HibernateEnabledDefault");
    }

    private static int? ReadInt(string path, string name, RegistryKey? hive = null)
    {
        try
        {
            using var k = (hive ?? Registry.LocalMachine).OpenSubKey(path);
            return k?.GetValue(name) is int v ? v : null;
        }
        catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException)
        {
            return null;
        }
    }

    private static string[]? ReadMulti(string path, string name)
    {
        try
        {
            using var k = Registry.LocalMachine.OpenSubKey(path);
            return k?.GetValue(name) as string[];
        }
        catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException)
        {
            return null;
        }
    }

    /// <summary>Менеджер области индексирования каталога SystemIndex (только чтение).</summary>
    private static ISearchCrawlScopeManager ScopeManager()
    {
        var type = Type.GetTypeFromCLSID(new Guid("7D096C5F-AC08-4F1F-BEB7-5C22C517CE39"), true)!;
        var manager = (ISearchManager)Activator.CreateInstance(type)!;
        try
        {
            Marshal.ThrowExceptionForHR(manager.GetCatalog("SystemIndex", out var catalog));
            try
            {
                Marshal.ThrowExceptionForHR(catalog.GetCrawlScopeManager(out var scope));
                return scope;
            }
            finally
            {
                Marshal.ReleaseComObject(catalog);
            }
        }
        finally
        {
            Marshal.ReleaseComObject(manager);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PropertyKey
    {
        public Guid FormatId;
        public uint PropertyId;
    }

    // Порядок методов как в ShObjIdl_core.h и SearchAPI.h; ненужные методы только держат место
    [ComImport, Guid("7e9fb0d3-919f-4307-ab2e-9b1860310c93"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellItem2
    {
        void BindToHandler(); void GetParent(); void GetDisplayName(); void GetAttributes(); void Compare();
        void GetPropertyStore(); void GetPropertyStoreWithCreateObject(); void GetPropertyStoreForKeys();
        void GetPropertyDescriptionList(); void Update(); void GetProperty(); void GetCLSID(); void GetFileTime();
        [PreserveSig] int GetInt32(ref PropertyKey key, out int value);
    }

    [ComImport, Guid("AB310581-AC80-11D1-8DF3-00C04FB6EF69"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ISearchManager
    {
        void GetIndexerVersionStr(); void GetIndexerVersion(); void GetParameter(); void SetParameter();
        void get_ProxyName(); void get_BypassList(); void SetProxy();
        [PreserveSig] int GetCatalog([MarshalAs(UnmanagedType.LPWStr)] string name, out ISearchCatalogManager catalog);
    }

    [ComImport, Guid("AB310581-AC80-11D1-8DF3-00C04FB6EF50"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ISearchCatalogManager
    {
        void get_Name(); void GetParameter(); void SetParameter(); void GetCatalogStatus(); void Reset(); void Reindex();
        void ReindexMatchingURLs(); void ReindexSearchRoot(); void put_ConnectTimeout(); void get_ConnectTimeout();
        void put_DataTimeout(); void get_DataTimeout(); void NumberOfItems(); void NumberOfItemsToIndex();
        void URLBeingIndexed(); void GetURLIndexingState(); void GetPersistentItemsChangedSink();
        void RegisterViewForNotification(); void GetItemsChangedSink(); void UnregisterViewForNotification();
        void SetExtensionClusion(); void EnumerateExcludedExtensions(); void GetQueryHelper();
        void put_DiacriticSensitivity(); void get_DiacriticSensitivity();
        [PreserveSig] int GetCrawlScopeManager(out ISearchCrawlScopeManager scope);
    }

    [ComImport, Guid("AB310581-AC80-11D1-8DF3-00C04FB6EF55"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ISearchCrawlScopeManager
    {
        void AddDefaultScopeRule(); void AddRoot(); void RemoveRoot(); void EnumerateRoots(); void AddHierarchicalScope();
        void AddUserScopeRule(); void RemoveScopeRule(); void EnumerateScopeRules(); void HasParentScopeRule();
        void HasChildScopeRule();
        [PreserveSig] int IncludedInCrawlScope([MarshalAs(UnmanagedType.LPWStr)] string url, out int included);
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHCreateItemFromParsingName(string path, IntPtr bc, ref Guid riid,
        [MarshalAs(UnmanagedType.Interface)] out IShellItem2? item);

    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    private static extern int PSGetPropertyKeyFromName(string name, out PropertyKey key);

    [DllImport("powrprof.dll")]
    [return: MarshalAs(UnmanagedType.U1)]
    private static extern bool GetPwrCapabilities(byte[] capabilities);
}
