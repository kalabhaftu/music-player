using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;
using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>Native thumbnail-toolbar transport controls shown when hovering the taskbar button.</summary>
internal sealed class TaskbarPeekControls : IDisposable
{
    private const uint WmCommand = 0x0111;
    private const uint WmAppCommand = 0x0319;
    private const uint ThbnClicked = 0x1800;
    private const uint ThbIcon = 0x0002;
    private const uint ThbTooltip = 0x0004;
    private const uint ThbFlags = 0x0008;
    private const uint ThbfDisabled = 0x0001;
    private const uint ImageColor = 0;
    private const uint BiRgb = 0;
    private const int IconSize = 32;

    private static readonly uint TaskbarButtonCreatedMessage = RegisterWindowMessage("TaskbarButtonCreated");
    private static readonly SubclassProcedure WindowProcedure = SubclassWindow;
    private static readonly nint PreviousIcon = CreateIcon(IconGlyph.Previous);
    private static readonly nint PlayIcon = CreateIcon(IconGlyph.Play);
    private static readonly nint PauseIcon = CreateIcon(IconGlyph.Pause);
    private static readonly nint NextIcon = CreateIcon(IconGlyph.Next);

    private readonly nint _hwnd;
    private readonly DispatcherQueue _dispatcher;
    private readonly Action _previous;
    private readonly Action _togglePlayback;
    private readonly Action _next;
    private readonly nuint _subclassId;
    private GCHandle _selfHandle;
    private ITaskbarList3? _taskbar;
    private bool _buttonsAdded;
    private bool _hasTrack;
    private bool _isPlaying;
    private bool _disposed;

    public TaskbarPeekControls(nint hwnd, DispatcherQueue dispatcher, Action previous, Action togglePlayback, Action next)
    {
        _hwnd = hwnd;
        _dispatcher = dispatcher;
        _previous = previous;
        _togglePlayback = togglePlayback;
        _next = next;
        _subclassId = unchecked((nuint)Interlocked.Increment(ref _nextSubclassId));
        _selfHandle = GCHandle.Alloc(this);
        if (!SetWindowSubclass(hwnd, WindowProcedure, _subclassId, GCHandle.ToIntPtr(_selfHandle)))
        {
            _selfHandle.Free();
            throw new COMException("The taskbar preview controls could not attach to the player window.", Marshal.GetLastWin32Error());
        }
    }

    private static long _nextSubclassId;

    public void Update(bool hasTrack, bool isPlaying)
    {
        _hasTrack = hasTrack;
        _isPlaying = isPlaying;
        if (_buttonsAdded) UpdateButtons();
    }

    private void OnTaskbarButtonCreated()
    {
        try
        {
            if (_taskbar is null)
            {
                var taskbarType = Type.GetTypeFromCLSID(new Guid("56FDF344-FD6D-11D0-958A-006097C9A090"), throwOnError: true)!;
                _taskbar = (ITaskbarList3)Activator.CreateInstance(taskbarType)!;
            }
            _taskbar.HrInit();
            var buttons = CreateButtons();
            _taskbar.ThumbBarAddButtons(_hwnd, (uint)buttons.Length, buttons);
            _buttonsAdded = true;
        }
        catch (Exception ex) when (ex is COMException or InvalidCastException or InvalidOperationException)
        {
            LocalAppLog.Shared.Warning("taskbar-controls", "Could not add playback controls to the taskbar preview.", ex);
        }
    }

    private void UpdateButtons()
    {
        try { _taskbar?.ThumbBarUpdateButtons(_hwnd, 3, CreateButtons()); }
        catch (Exception ex) when (ex is COMException or InvalidOperationException)
        { LocalAppLog.Shared.Warning("taskbar-controls", "Could not refresh taskbar preview playback controls.", ex); }
    }

    private ThumbButton[] CreateButtons() =>
    [
        Button(1, PreviousIcon, "Previous track"),
        Button(2, _isPlaying ? PauseIcon : PlayIcon, _isPlaying ? "Pause" : "Play"),
        Button(3, NextIcon, "Next track")
    ];

    private ThumbButton Button(uint id, nint icon, string tooltip) => new()
    {
        Mask = ThbIcon | ThbTooltip | ThbFlags,
        Id = id,
        Icon = icon,
        Tooltip = tooltip,
        Flags = _hasTrack ? 0u : ThbfDisabled
    };

    private bool HandleCommand(nint wParam)
    {
        var command = unchecked((ulong)wParam.ToInt64());
        if (((command >> 16) & 0xffff) != ThbnClicked) return false;
        Action? callback = (command & 0xffff) switch { 1 => _previous, 2 => _togglePlayback, 3 => _next, _ => null };
        if (callback is null) return false;
        _dispatcher.TryEnqueue(() => callback());
        return true;
    }

    private bool HandleMediaKey(nint lParam)
    {
        // GET_APPCOMMAND_LPARAM: exclude the input-device bits in HIWORD.
        var command = (unchecked((ulong)lParam.ToInt64()) >> 16) & 0x0fff;
        Action? callback = command switch
        {
            11 => _next,
            12 => _previous,
            14 => _togglePlayback,
            46 => () => { if (!_isPlaying) _togglePlayback(); },
            47 => () => { if (_isPlaying) _togglePlayback(); },
            _ => null
        };
        if (callback is null) return false;
        _dispatcher.TryEnqueue(() => callback());
        return true;
    }

    private static nint SubclassWindow(nint hwnd, uint message, nint wParam, nint lParam, nuint subclassId, nint referenceData)
    {
        var handle = GCHandle.FromIntPtr(referenceData);
        if (handle.Target is TaskbarPeekControls owner)
        {
            if (message == TaskbarButtonCreatedMessage) owner.OnTaskbarButtonCreated();
            else if (message == WmCommand && owner.HandleCommand(wParam)) return 0;
            // Returning TRUE prevents an already-handled key from also reaching
            // the shell's global media controller and toggling playback twice.
            else if (message == WmAppCommand && owner.HandleMediaKey(lParam)) return 1;
        }
        return DefSubclassProc(hwnd, message, wParam, lParam);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        RemoveWindowSubclass(_hwnd, WindowProcedure, _subclassId);
        if (_selfHandle.IsAllocated) _selfHandle.Free();
        if (_taskbar is not null && Marshal.IsComObject(_taskbar)) Marshal.ReleaseComObject(_taskbar);
        _taskbar = null;
    }

    private enum IconGlyph { Previous, Play, Pause, Next }

    private static nint CreateIcon(IconGlyph glyph)
    {
        var pixels = new byte[IconSize * IconSize * 4];
        const int samples = 4;
        for (var y = 0; y < IconSize; y++)
        for (var x = 0; x < IconSize; x++)
        {
            var covered = 0;
            for (var sy = 0; sy < samples; sy++)
            for (var sx = 0; sx < samples; sx++)
                if (Contains(glyph, x + (sx + .5) / samples, y + (sy + .5) / samples)) covered++;
            if (covered == 0) continue;
            var offset = (y * IconSize + x) * 4;
            pixels[offset] = pixels[offset + 1] = pixels[offset + 2] = 255;
            pixels[offset + 3] = (byte)(covered * 255 / (samples * samples));
        }

        var info = new BitmapInfo
        {
            Header = new BitmapInfoHeader
            {
                Size = (uint)Marshal.SizeOf<BitmapInfoHeader>(), Width = IconSize, Height = -IconSize,
                Planes = 1, BitCount = 32, Compression = BiRgb
            }
        };
        var colorBitmap = CreateDIBSection(0, ref info, ImageColor, out var pixelData, 0, 0);
        if (colorBitmap == 0 || pixelData == 0) return 0;
        Marshal.Copy(pixels, 0, pixelData, pixels.Length);
        var mask = CreateBitmap(IconSize, IconSize, 1, 1, new byte[IconSize * IconSize / 8]);
        if (mask == 0) { DeleteObject(colorBitmap); return 0; }
        var iconInfo = new IconInfo { IsIcon = true, MaskBitmap = mask, ColorBitmap = colorBitmap };
        var icon = CreateIconIndirect(ref iconInfo);
        DeleteObject(mask);
        DeleteObject(colorBitmap);
        return icon;
    }

    private static bool Contains(IconGlyph glyph, double x, double y)
    {
        static bool Rect(double px, double py, double left, double top, double right, double bottom)
            => px >= left && px <= right && py >= top && py <= bottom;
        static bool Triangle(double px, double py, double ax, double ay, double bx, double by, double cx, double cy)
        {
            static double Side(double px, double py, double ax, double ay, double bx, double by)
                => (px - bx) * (ay - by) - (ax - bx) * (py - by);
            var a = Side(px, py, ax, ay, bx, by); var b = Side(px, py, bx, by, cx, cy); var c = Side(px, py, cx, cy, ax, ay);
            return (a >= 0 && b >= 0 && c >= 0) || (a <= 0 && b <= 0 && c <= 0);
        }
        return glyph switch
        {
            IconGlyph.Play => Triangle(x, y, 8, 5, 8, 27, 27, 16),
            IconGlyph.Pause => Rect(x, y, 7, 5, 13, 27) || Rect(x, y, 19, 5, 25, 27),
            IconGlyph.Previous => Rect(x, y, 3, 7, 6, 25) || Triangle(x, y, 7, 16, 18, 7, 18, 25) || Triangle(x, y, 15, 16, 27, 7, 27, 25),
            IconGlyph.Next => Rect(x, y, 26, 7, 29, 25) || Triangle(x, y, 17, 7, 17, 25, 28, 16) || Triangle(x, y, 7, 7, 7, 25, 19, 16),
            _ => false
        };
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct ThumbButton
    {
        public uint Mask;
        public uint Id;
        public uint BitmapIndex;
        public nint Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string? Tooltip;
        public uint Flags;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IconInfo { [MarshalAs(UnmanagedType.Bool)] public bool IsIcon; public uint HotspotX; public uint HotspotY; public nint MaskBitmap; public nint ColorBitmap; }
    [StructLayout(LayoutKind.Sequential)]
    private struct BitmapInfo { public BitmapInfoHeader Header; public uint Color; }
    [StructLayout(LayoutKind.Sequential)]
    private struct BitmapInfoHeader
    {
        public uint Size; public int Width; public int Height; public ushort Planes; public ushort BitCount;
        public uint Compression; public uint ImageSize; public int XPelsPerMeter; public int YPelsPerMeter;
        public uint ColorsUsed; public uint ColorsImportant;
    }

    [ComImport, Guid("EA1AFB91-9E28-4B86-90E9-9E9F8A5EEFAF"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITaskbarList3
    {
        void HrInit(); void AddTab(nint hwnd); void DeleteTab(nint hwnd); void ActivateTab(nint hwnd); void SetActiveAlt(nint hwnd);
        void MarkFullscreenWindow(nint hwnd, [MarshalAs(UnmanagedType.Bool)] bool fullscreen);
        void SetProgressValue(nint hwnd, ulong completed, ulong total); void SetProgressState(nint hwnd, uint flags);
        void RegisterTab(nint tab, nint mdi); void UnregisterTab(nint tab); void SetTabOrder(nint tab, nint insertBefore); void SetTabActive(nint tab, nint mdi, uint reserved);
        void ThumbBarAddButtons(nint hwnd, uint count, [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 1)] ThumbButton[] buttons);
        void ThumbBarUpdateButtons(nint hwnd, uint count, [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 1)] ThumbButton[] buttons);
        void ThumbBarSetImageList(nint hwnd, nint imageList);
        void SetOverlayIcon(nint hwnd, nint icon, [MarshalAs(UnmanagedType.LPWStr)] string? description);
        void SetThumbnailTooltip(nint hwnd, [MarshalAs(UnmanagedType.LPWStr)] string tooltip);
        void SetThumbnailClip(nint hwnd, ref NativeRect clip);
    }

    [StructLayout(LayoutKind.Sequential)] private struct NativeRect { public int Left, Top, Right, Bottom; }
    [UnmanagedFunctionPointer(CallingConvention.Winapi)] private delegate nint SubclassProcedure(nint hwnd, uint message, nint wParam, nint lParam, nuint subclassId, nint referenceData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern uint RegisterWindowMessage(string message);
    [DllImport("comctl32.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool SetWindowSubclass(nint hwnd, SubclassProcedure procedure, nuint subclassId, nint referenceData);
    [DllImport("comctl32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool RemoveWindowSubclass(nint hwnd, SubclassProcedure procedure, nuint subclassId);
    [DllImport("comctl32.dll")] private static extern nint DefSubclassProc(nint hwnd, uint message, nint wParam, nint lParam);
    [DllImport("gdi32.dll", SetLastError = true)] private static extern nint CreateDIBSection(nint hdc, ref BitmapInfo info, uint usage, out nint bits, nint section, uint offset);
    [DllImport("gdi32.dll", SetLastError = true)] private static extern nint CreateBitmap(int width, int height, uint planes, uint bitsPerPixel, [In] byte[] bits);
    [DllImport("gdi32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool DeleteObject(nint handle);
    [DllImport("user32.dll", SetLastError = true)] private static extern nint CreateIconIndirect(ref IconInfo info);
    [DllImport("user32.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool DestroyIcon(nint icon);
}
