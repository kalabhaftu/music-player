using System.ComponentModel;
using System.Runtime.InteropServices;

namespace MusicPlayer.App;

internal sealed class TrayIconService : IDisposable
{
    private const uint WmSize = 0x0005, WmClose = 0x0010, WmQueryEndSession = 0x0011, WmEndSession = 0x0016;
    private const uint WmNull = 0x0000, WmApp = 0x8000, WmContextMenu = 0x007B, WmLButtonUp = 0x0202, WmLButtonDoubleClick = 0x0203, WmRButtonUp = 0x0205;
    private const uint NinSelect = 0x0400, NinKeySelect = 0x0401;
    private const uint SizeMinimized = 1, SwHide = 0, SwRestore = 9, ImageIcon = 1, LoadFromFile = 0x10;
    private const uint NimAdd = 0, NimModify = 1, NimDelete = 2, NimSetVersion = 4, NifMessage = 1, NifIcon = 2, NifTip = 4;
    private const uint TpmRightButton = 0x0002, TpmNonotify = 0x0080, TpmReturnCommand = 0x0100;
    private const uint MfSeparator = 0x0800;
    private const uint OpenCommand = 1, PreviousCommand = 2, PlayPauseCommand = 3, NextCommand = 4, QuitCommand = 5;
    private const uint CallbackMessage = WmApp + 41;
    private static readonly uint TaskbarCreatedMessage = RegisterWindowMessage("TaskbarCreated");
    private readonly IntPtr _window;
    private IntPtr _icon;
    private readonly Action _restore;
    private readonly Action _playPause;
    private readonly Action _previous;
    private readonly Action _next;
    private readonly Action _quit;
    private readonly SubclassProcedure _procedure;
    private bool _disposed;
    private bool _registered;
    private bool _systemSessionEnding;

    public TrayIconService(IntPtr window, string iconPath, Action restore, Action playPause, Action previous, Action next, Action quit)
    {
        _window = window; _restore = restore; _playPause = playPause; _previous = previous; _next = next; _quit = quit; _procedure = WindowProcedure;
        _icon = LoadSmallIcon(iconPath);
        if (_icon == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not load the Music Player tray icon.");
        if (!SetWindowSubclass(_window, _procedure, (UIntPtr)0xB4A7, UIntPtr.Zero))
        { DestroyIcon(_icon); throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not attach the tray icon to the app window."); }
        var data = MakeData();
        if (!Shell_NotifyIcon(NimAdd, ref data))
        {
            var error = Marshal.GetLastWin32Error();
            RemoveWindowSubclass(_window, _procedure, (UIntPtr)0xB4A7); DestroyIcon(_icon);
            throw new Win32Exception(error, $"Windows could not register the tray icon (native error {error}, data size {data.cbSize}).");
        }
        _registered = true;
        data.uTimeoutOrVersion = 4;
        Shell_NotifyIcon(NimSetVersion, ref data);
    }

    public static void RestoreWindow(IntPtr window)
    {
        ShowWindow(window, SwRestore);
        SetForegroundWindow(window);
    }

    public void UpdateIcon(string iconPath)
    {
        var next = LoadSmallIcon(iconPath);
        if (next == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not load the Music Player tray icon.");
        var previous = _icon;
        _icon = next;
        var data = MakeData();
        if (!Shell_NotifyIcon(NimModify, ref data))
        {
            _icon = previous;
            DestroyIcon(next);
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Windows could not update the Music Player tray icon.");
        }
        DestroyIcon(previous);
    }

    private NotifyIconData MakeData() => new()
    {
        cbSize = (uint)Marshal.SizeOf<NotifyIconData>(), hWnd = _window, uID = 1,
        uFlags = NifMessage | NifIcon | NifTip, uCallbackMessage = CallbackMessage, hIcon = _icon,
        szTip = "Music Player", szInfo = string.Empty, szInfoTitle = string.Empty
    };

    private static IntPtr LoadSmallIcon(string path) =>
        LoadImage(IntPtr.Zero, path, ImageIcon, GetSystemMetrics(49), GetSystemMetrics(50), LoadFromFile);

    private IntPtr WindowProcedure(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, UIntPtr subclassId, UIntPtr referenceData)
    {
        if (message == WmQueryEndSession)
        {
            _systemSessionEnding = true;
            return DefSubclassProc(window, message, wParam, lParam);
        }
        if (message == WmEndSession)
        {
            _systemSessionEnding = wParam != UIntPtr.Zero;
            return DefSubclassProc(window, message, wParam, lParam);
        }
        if (message == WmClose && !_systemSessionEnding)
        {
            ShowWindow(window, SwHide);
            return IntPtr.Zero;
        }
        if (message == TaskbarCreatedMessage)
        {
            // Explorer restart and primary-display DPI changes remove tray icons.
            var data = MakeData();
            _registered = Shell_NotifyIcon(NimAdd, ref data);
            if (_registered) { data.uTimeoutOrVersion = 4; Shell_NotifyIcon(NimSetVersion, ref data); }
            else ShowWindow(window, SwRestore);
        }
        if (_registered && message == WmSize && wParam.ToUInt32() == SizeMinimized)
        {
            ShowWindow(window, SwHide);
            return IntPtr.Zero;
        }
        var notification = unchecked((uint)lParam.ToInt64()) & 0xFFFF;
        if (message == CallbackMessage && notification is WmContextMenu or WmRButtonUp)
        {
            ShowContextMenu();
            return IntPtr.Zero;
        }
        if (message == CallbackMessage && notification is WmLButtonUp or WmLButtonDoubleClick or NinSelect or NinKeySelect)
        {
            _restore();
            return IntPtr.Zero;
        }
        return DefSubclassProc(window, message, wParam, lParam);
    }

    private void ShowContextMenu()
    {
        var menu = CreatePopupMenu();
        if (menu == IntPtr.Zero) { _restore(); return; }
        try
        {
            if (!AppendMenu(menu, 0, OpenCommand, "Open Music Player") ||
                !AppendMenu(menu, MfSeparator, 0, string.Empty) ||
                !AppendMenu(menu, 0, PreviousCommand, "Previous") ||
                !AppendMenu(menu, 0, PlayPauseCommand, "Play / Pause") ||
                !AppendMenu(menu, 0, NextCommand, "Next") ||
                !AppendMenu(menu, MfSeparator, 0, string.Empty) ||
                !AppendMenu(menu, 0, QuitCommand, "Quit"))
            {
                _restore();
                return;
            }

            if (!GetCursorPos(out var point)) { _restore(); return; }
            SetForegroundWindow(_window);
            var command = TrackPopupMenuEx(menu, TpmRightButton | TpmNonotify | TpmReturnCommand,
                point.X, point.Y, _window, IntPtr.Zero);
            PostMessage(_window, WmNull, UIntPtr.Zero, IntPtr.Zero);
            switch (command)
            {
                case OpenCommand: _restore(); break;
                case PreviousCommand: _previous(); break;
                case PlayPauseCommand: _playPause(); break;
                case NextCommand: _next(); break;
                case QuitCommand: _quit(); break;
            }
        }
        finally { DestroyMenu(menu); }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        var data = MakeData(); Shell_NotifyIcon(NimDelete, ref data);
        RemoveWindowSubclass(_window, _procedure, (UIntPtr)0xB4A7);
        DestroyIcon(_icon);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public uint uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X;
        public int Y;
    }

    private delegate IntPtr SubclassProcedure(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, UIntPtr subclassId, UIntPtr referenceData);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool Shell_NotifyIcon(uint message, ref NotifyIconData data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr LoadImage(IntPtr instance, string name, uint type, int width, int height, uint flags);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool DestroyIcon(IntPtr icon);
    [DllImport("user32.dll")] private static extern bool ShowWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr CreatePopupMenu();
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "AppendMenuW", SetLastError = true)] private static extern bool AppendMenu(IntPtr menu, uint flags, uint itemId, string itemText);
    [DllImport("user32.dll")] private static extern uint TrackPopupMenuEx(IntPtr menu, uint flags, int x, int y, IntPtr owner, IntPtr parameters);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool DestroyMenu(IntPtr menu);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool PostMessage(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern uint RegisterWindowMessage(string message);
    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
    [DllImport("comctl32.dll", SetLastError = true)] private static extern bool SetWindowSubclass(IntPtr window, SubclassProcedure procedure, UIntPtr subclassId, UIntPtr referenceData);
    [DllImport("comctl32.dll", SetLastError = true)] private static extern bool RemoveWindowSubclass(IntPtr window, SubclassProcedure procedure, UIntPtr subclassId);
    [DllImport("comctl32.dll")] private static extern IntPtr DefSubclassProc(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);
}
