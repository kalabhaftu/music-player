using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace MusicPlayer.App;

internal static class DefaultAppsService
{
    internal static readonly string[] AudioExtensions = [".mp3", ".flac", ".wav", ".wave", ".aif", ".aiff", ".m4a", ".m4b", ".aac", ".ogg", ".oga", ".opus", ".wma", ".ape", ".wv", ".tta", ".mpc", ".dsf", ".dff"];

    public static bool NeedsSelection()
    {
        object? instance = null;
        try
        {
            instance = Activator.CreateInstance(Type.GetTypeFromCLSID(new("591209C7-767B-42B2-9FBA-44EE4615F2C7"), throwOnError: true)!);
            var registration = (IAssociationRegistration)instance!;
            string? aumid = null;
            try { if (OperatingSystem.IsWindowsVersionAtLeast(10, 0, 19041)) aumid = Windows.ApplicationModel.AppInfo.Current.AppUserModelId; } catch (InvalidOperationException) { } catch (COMException) { }
            foreach (var extension in AudioExtensions)
            {
                if (registration.QueryCurrentDefault(extension, 0, 1, out var progId) < 0) return true;
                if (string.Equals(progId, "MusicPlayer.MusicPlayer." + extension[1..], StringComparison.OrdinalIgnoreCase)) continue;
                if (string.IsNullOrWhiteSpace(progId)) return true;
                using var app = Registry.ClassesRoot.OpenSubKey(progId + "\\Application");
                if (aumid is null || !string.Equals(app?.GetValue("AppUserModelID") as string, aumid, StringComparison.OrdinalIgnoreCase)) return true;
            }
            return false;
        }
        catch (COMException) { return true; }
        finally { if (instance is not null && Marshal.IsComObject(instance)) Marshal.ReleaseComObject(instance); }
    }

    public static async Task OpenAsync()
    {
        var uri = "ms-settings:defaultapps";
        if (OperatingSystem.IsWindowsVersionAtLeast(10, 0, 22631))
        {
            try { uri += "?registeredAUMID=" + Uri.EscapeDataString(Windows.ApplicationModel.AppInfo.Current.AppUserModelId); }
            catch (Exception ex) when (ex is InvalidOperationException or COMException)
            { uri += "?registeredAppUser=" + Uri.EscapeDataString("Music Player"); }
        }
        if (!await Windows.System.Launcher.LaunchUriAsync(new(uri)))
            await Windows.System.Launcher.LaunchUriAsync(new("ms-settings:defaultapps"));
    }

    [ComImport, Guid("4E530B0A-E611-4C77-A3AC-9031D022281B"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IAssociationRegistration
    {
        [PreserveSig] int QueryCurrentDefault([MarshalAs(UnmanagedType.LPWStr)] string query, int type, int level,
            [MarshalAs(UnmanagedType.LPWStr)] out string progId);
    }
}
