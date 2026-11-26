using System.Runtime.InteropServices;

namespace MusicPlayer.App;

/// <summary>Owns the child HWND LibVLC uses so video stays inside the player window.</summary>
internal sealed class VideoSurfaceHost : IDisposable
{
    private const uint Child = 0x40000000;
    private const uint ClipSiblings = 0x04000000;
    private const uint ClipChildren = 0x02000000;
    private const uint ShowWithoutActivating = 0x0010;
    private const uint ShowWindow = 0x0040;
    private const int HideWindowCommand = 0;
    private int _disposed;

    public VideoSurfaceHost(nint parent)
    {
        Handle = CreateWindowEx(0, "STATIC", string.Empty, Child | ClipSiblings | ClipChildren,
            0, 0, 1, 1, parent, 0, 0, 0);
        if (Handle == 0) throw new InvalidOperationException("The in-app video surface could not be created.");
    }

    public nint Handle { get; private set; }

    public void SetBounds(double x, double y, double width, double height, bool visible)
    {
        if (Handle == 0 || Volatile.Read(ref _disposed) != 0) return;
        if (!visible || width < 2 || height < 2)
        {
            ShowWindowNative(Handle, HideWindowCommand);
            return;
        }

        var dpi = GetDpiForWindow(GetParent(Handle));
        var scale = dpi is 0 ? 1d : dpi / 96d;
        var left = (int)Math.Round(x * scale);
        var top = (int)Math.Round(y * scale);
        var pixelWidth = Math.Max(2, (int)Math.Round(width * scale));
        var pixelHeight = Math.Max(2, (int)Math.Round(height * scale));
        if (!SetWindowPos(Handle, 0, left, top, pixelWidth, pixelHeight, ShowWithoutActivating | ShowWindow))
            throw new InvalidOperationException("The in-app video surface could not be positioned.");
    }

    public void Hide()
    {
        if (Handle != 0 && Volatile.Read(ref _disposed) == 0) ShowWindowNative(Handle, HideWindowCommand);
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        if (Handle != 0) DestroyWindow(Handle);
        Handle = 0;
    }

    [DllImport("user32.dll", EntryPoint = "CreateWindowExW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern nint CreateWindowEx(uint extendedStyle, string className, string windowName, uint style,
        int x, int y, int width, int height, nint parent, nint menu, nint instance, nint parameter);

    [DllImport("user32.dll", EntryPoint = "SetWindowPos", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(nint window, nint insertAfter, int x, int y, int width, int height, uint flags);

    [DllImport("user32.dll", EntryPoint = "ShowWindow", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ShowWindowNative(nint window, int command);

    [DllImport("user32.dll", EntryPoint = "DestroyWindow", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyWindow(nint window);

    [DllImport("user32.dll", EntryPoint = "GetParent", SetLastError = true)]
    private static extern nint GetParent(nint window);

    [DllImport("user32.dll", EntryPoint = "GetDpiForWindow", SetLastError = true)]
    private static extern uint GetDpiForWindow(nint window);
}
