using System.Runtime.InteropServices;

namespace MusicPlayer.App;

/// <summary>Maps the saved Windows endpoint to VLC's DirectSound device identifier.</summary>
internal static class AudioOutputRouting
{
    internal static string EndpointId(string id)
    {
        // WinRT device-interface IDs wrap the MMDevice endpoint ID in # sections.
        var start = id.IndexOf("{0.0.0.", StringComparison.OrdinalIgnoreCase);
        if (start < 0) return id;
        var end = id.IndexOf('#', start);
        return end < 0 ? id[start..] : id[start..end];
    }

    internal static string DirectSoundDeviceId(string id)
    {
        if (string.IsNullOrWhiteSpace(id)) return "";
        // An already mapped DirectSound GUID needs no COM lookup.
        if (Guid.TryParse(id, out var directSound)) return directSound.ToString("B");
        IMMDeviceEnumerator? enumerator = null;
        IMMDevice? device = null;
        IPropertyStore? properties = null;
        var value = new PropVariant();
        var initialized = CoInitializeEx(0, 0);
        try
        {
            enumerator = (IMMDeviceEnumerator)new MMDeviceEnumerator();
            Marshal.ThrowExceptionForHR(enumerator.GetDevice(EndpointId(id), out device));
            Marshal.ThrowExceptionForHR(device.GetState(out var state));
            if ((state & 1) == 0) return ""; // DEVICE_STATE_ACTIVE; disconnected endpoints remain registered.
            Marshal.ThrowExceptionForHR(device.OpenPropertyStore(0, out properties));
            var key = new PropertyKey { FormatId = new("1da5d803-d492-4edd-8c23-e0c0ffee7f0e"), Id = 4 };
            Marshal.ThrowExceptionForHR(properties.GetValue(ref key, out value));
            if (value.Type == 31 && Guid.TryParse(Marshal.PtrToStringUni(value.Pointer), out directSound))
                return directSound.ToString("B");
            throw new InvalidOperationException("The audio endpoint has no DirectSound identifier.");
        }
        catch (Exception ex) when (ex is COMException or InvalidOperationException or ArgumentException)
        {
            MusicPlayer.Core.LocalAppLog.Shared.Warning("audio-devices", "The saved audio output is unavailable; using the system default.", ex);
            return "";
        }
        finally
        {
            PropVariantClear(ref value);
            if (properties is not null) Marshal.ReleaseComObject(properties);
            if (device is not null) Marshal.ReleaseComObject(device);
            if (enumerator is not null) Marshal.ReleaseComObject(enumerator);
            if (initialized >= 0) CoUninitialize();
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PropertyKey { public Guid FormatId; public uint Id; }
    [StructLayout(LayoutKind.Explicit, Size = 24)]
    private struct PropVariant
    {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(8)] public nint Pointer;
    }

    [DllImport("ole32.dll")] private static extern int CoInitializeEx(nint reserved, uint flags);
    [DllImport("ole32.dll")] private static extern void CoUninitialize();
    [DllImport("ole32.dll")] private static extern int PropVariantClear(ref PropVariant value);

    [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
    private class MMDeviceEnumerator { }

    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDeviceEnumerator
    {
        [PreserveSig] int EnumAudioEndpoints(int flow, uint mask, out nint devices);
        [PreserveSig] int GetDefaultAudioEndpoint(int flow, int role, out IMMDevice device);
        [PreserveSig] int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IMMDevice device);
    }

    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDevice
    {
        [PreserveSig] int Activate(ref Guid iid, uint context, nint parameters, out nint instance);
        [PreserveSig] int OpenPropertyStore(uint access, out IPropertyStore properties);
        [PreserveSig] int GetId(out nint id);
        [PreserveSig] int GetState(out uint state);
    }

    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetAt(uint index, out PropertyKey key);
        [PreserveSig] int GetValue(ref PropertyKey key, out PropVariant value);
    }
}
