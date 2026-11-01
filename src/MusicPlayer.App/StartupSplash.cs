using System.Runtime.InteropServices;

namespace MusicPlayer.App;

// A per-pixel-alpha native window: only the app artwork and text are visible.
// It does not create a second XAML tree, taskbar button, border, or background.
internal sealed class StartupSplash : IDisposable
{
    private readonly nint _window;
    private static readonly WindowProcedure Procedure = DefWindowProc;
    private delegate nint WindowProcedure(nint window, uint message, nint wParam, nint lParam);

    public static StartupSplash? TryShow()
    {
        try { return new StartupSplash(); }
        catch (Exception ex) { MusicPlayer.Core.LocalAppLog.Shared.Warning("startup", "Could not show startup artwork.", ex); return null; }
    }

    private StartupSplash()
    {
        var instance = GetModuleHandle(null);
        var windowClass = new WindowClass { Size = (uint)Marshal.SizeOf<WindowClass>(), Procedure = Procedure,
            Instance = instance, ClassName = "MusicPlayer.StartupArtwork" };
        if (RegisterClassEx(ref windowClass) == 0) throw new System.ComponentModel.Win32Exception();
        _window = CreateWindowEx(0x00080088, windowClass.ClassName, "Music Player", 0x80000000,
            0, 0, 1, 1, 0, 0, instance, 0);
        if (_window == 0) throw new System.ComponentModel.Win32Exception();
        try { Render("Opening"); }
        catch { DestroyWindow(_window); throw; }
    }

    public void SetStatus(string status)
    {
        try { Render(status); }
        catch (Exception ex) { MusicPlayer.Core.LocalAppLog.Shared.Warning("startup", "Could not update startup status.", ex); }
    }

    private void Render(string status)
    {
        var scale = GetDpiForWindow(_window) / 96f;
        var width = (int)(240 * scale); var height = (int)(224 * scale);
        var info = new BitmapInfo { HeaderSize = 40, Width = width, Height = -height, Planes = 1, BitCount = 32 };
        var screen = GetDC(0); var dc = CreateCompatibleDC(screen);
        var bitmap = CreateDIBSection(screen, ref info, 0, out var pixels, 0, 0);
        if (bitmap == 0 || dc == 0) { if (bitmap != 0) DeleteObject(bitmap); if (dc != 0) DeleteDC(dc); ReleaseDC(0, screen); throw new System.ComponentModel.Win32Exception(); }
        var previous = SelectObject(dc, bitmap);
        nuint token = 0; nint image = 0, graphics = 0, family = 0, font = 0, brush = 0, shadow = 0, format = 0;
        try
        {
            var input = new GdiStartup { Version = 1 };
            Check(GdiplusStartup(out token, ref input, 0));
            Check(GdipCreateBitmapFromScan0(width, height, width * 4, 0x000E200B, pixels, out image));
            Check(GdipGetImageGraphicsContext(image, out graphics));
            Check(GdipGraphicsClear(graphics, 0));
            nint icon = 0;
            try
            {
                Check(GdipLoadImageFromFile(Path.Combine(AppContext.BaseDirectory, "Assets", "MusicPlayer-dark.png"), out icon));
                Check(GdipSetInterpolationMode(graphics, 7));
                var size = (int)(160 * scale);
                Check(GdipDrawImageRectI(graphics, icon, (width - size) / 2, 4, size, size));
            }
            finally { if (icon != 0) GdipDisposeImage(icon); }
            Check(GdipCreateFontFamilyFromName("Segoe UI", 0, out family));
            Check(GdipCreateFont(family, 17 * scale, 1, 2, out font));
            Check(GdipCreateSolidFill(0xFFF5F7F4, out brush));
            Check(GdipCreateSolidFill(0xD0000000, out shadow));
            Check(GdipCreateStringFormat(0, 0, out format));
            Check(GdipSetStringFormatAlign(format, 1));
            Check(GdipSetTextRenderingHint(graphics, 4));
            var text = new FloatRectangle { X = 1, Y = 177 * scale + 1, Width = width, Height = 34 * scale };
            Check(GdipDrawString(graphics, status, -1, font, ref text, format, shadow));
            text.X = 0; text.Y -= 1;
            Check(GdipDrawString(graphics, status, -1, font, ref text, format, brush));
            Check(GdipFlush(graphics, 1));
            var monitor = MonitorFromWindow(GetForegroundWindow(), 2);
            var monitorInfo = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
            GetMonitorInfo(monitor, ref monitorInfo);
            var position = new Point { X = (monitorInfo.Work.Left + monitorInfo.Work.Right - width) / 2,
                Y = (monitorInfo.Work.Top + monitorInfo.Work.Bottom - height) / 2 };
            var origin = new Point(); var dimensions = new Size { Width = width, Height = height };
            var blend = new Blend { Operation = 0, ConstantAlpha = 255, AlphaFormat = 1 };
            if (!UpdateLayeredWindow(_window, screen, ref position, ref dimensions, dc, ref origin, 0, ref blend, 2))
                throw new System.ComponentModel.Win32Exception();
            ShowWindow(_window, 4);
        }
        finally
        {
            if (format != 0) GdipDeleteStringFormat(format);
            if (shadow != 0) GdipDeleteBrush(shadow);
            if (brush != 0) GdipDeleteBrush(brush);
            if (font != 0) GdipDeleteFont(font);
            if (family != 0) GdipDeleteFontFamily(family);
            if (graphics != 0) GdipDeleteGraphics(graphics);
            if (image != 0) GdipDisposeImage(image);
            if (token != 0) GdiplusShutdown(token);
            SelectObject(dc, previous); DeleteObject(bitmap); DeleteDC(dc); ReleaseDC(0, screen);
        }
    }

    private static void Check(int status) { if (status != 0) throw new InvalidOperationException($"Startup artwork rendering failed ({status})."); }
    public void Dispose() { if (_window != 0) DestroyWindow(_window); }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] private struct WindowClass { public uint Size, Style; public WindowProcedure Procedure; public int ClassExtra, WindowExtra; public nint Instance, Icon, Cursor, Background; public string? MenuName; public string ClassName; public nint SmallIcon; }
    [StructLayout(LayoutKind.Sequential)] private struct GdiStartup { public uint Version; public nint Callback; public int BackgroundThread, ExternalCodecs; }
    [StructLayout(LayoutKind.Sequential)] private struct BitmapInfo { public uint HeaderSize; public int Width, Height; public ushort Planes, BitCount; public uint Compression, ImageSize; public int XResolution, YResolution; public uint ColorsUsed, ImportantColors; }
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] private struct Size { public int Width, Height; }
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] private struct MonitorInfo { public int Size; public Rectangle Bounds, Work; public uint Flags; }
    [StructLayout(LayoutKind.Sequential)] private struct FloatRectangle { public float X, Y, Width, Height; }
    [StructLayout(LayoutKind.Sequential, Pack = 1)] private struct Blend { public byte Operation, Flags, ConstantAlpha, AlphaFormat; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern nint GetModuleHandle(string? name);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern ushort RegisterClassEx(ref WindowClass windowClass);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern nint CreateWindowEx(uint extendedStyle, string className, string title, uint style, int x, int y, int width, int height, nint parent, nint menu, nint instance, nint parameter);
    [DllImport("user32.dll", EntryPoint = "DefWindowProcW")] private static extern nint DefWindowProc(nint window, uint message, nint wParam, nint lParam);
    [DllImport("user32.dll")] private static extern bool DestroyWindow(nint window);
    [DllImport("user32.dll")] private static extern bool ShowWindow(nint window, int command);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(nint window);
    [DllImport("user32.dll")] private static extern nint GetForegroundWindow();
    [DllImport("user32.dll")] private static extern nint MonitorFromWindow(nint window, uint flags);
    [DllImport("user32.dll", EntryPoint = "GetMonitorInfoW")] private static extern bool GetMonitorInfo(nint monitor, ref MonitorInfo info);
    [DllImport("user32.dll")] private static extern nint GetDC(nint window);
    [DllImport("user32.dll")] private static extern int ReleaseDC(nint window, nint dc);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool UpdateLayeredWindow(nint window, nint destination, ref Point position, ref Size size, nint source, ref Point origin, uint colorKey, ref Blend blend, uint flags);
    [DllImport("gdi32.dll")] private static extern nint CreateCompatibleDC(nint dc);
    [DllImport("gdi32.dll")] private static extern nint CreateDIBSection(nint dc, ref BitmapInfo info, uint usage, out nint pixels, nint section, uint offset);
    [DllImport("gdi32.dll")] private static extern nint SelectObject(nint dc, nint item);
    [DllImport("gdi32.dll")] private static extern bool DeleteObject(nint item);
    [DllImport("gdi32.dll")] private static extern bool DeleteDC(nint dc);
    [DllImport("gdiplus.dll")] private static extern int GdiplusStartup(out nuint token, ref GdiStartup input, nint output);
    [DllImport("gdiplus.dll")] private static extern void GdiplusShutdown(nuint token);
    [DllImport("gdiplus.dll")] private static extern int GdipCreateBitmapFromScan0(int width, int height, int stride, int format, nint pixels, out nint bitmap);
    [DllImport("gdiplus.dll", CharSet = CharSet.Unicode)] private static extern int GdipLoadImageFromFile(string path, out nint image);
    [DllImport("gdiplus.dll")] private static extern int GdipGetImageGraphicsContext(nint image, out nint graphics);
    [DllImport("gdiplus.dll")] private static extern int GdipGraphicsClear(nint graphics, uint color);
    [DllImport("gdiplus.dll")] private static extern int GdipSetInterpolationMode(nint graphics, int mode);
    [DllImport("gdiplus.dll")] private static extern int GdipDrawImageRectI(nint graphics, nint image, int x, int y, int width, int height);
    [DllImport("gdiplus.dll", CharSet = CharSet.Unicode)] private static extern int GdipCreateFontFamilyFromName(string name, nint collection, out nint family);
    [DllImport("gdiplus.dll")] private static extern int GdipCreateFont(nint family, float size, int style, int unit, out nint font);
    [DllImport("gdiplus.dll")] private static extern int GdipCreateSolidFill(uint color, out nint brush);
    [DllImport("gdiplus.dll")] private static extern int GdipCreateStringFormat(int attributes, ushort language, out nint format);
    [DllImport("gdiplus.dll")] private static extern int GdipSetStringFormatAlign(nint format, int alignment);
    [DllImport("gdiplus.dll")] private static extern int GdipSetTextRenderingHint(nint graphics, int hint);
    [DllImport("gdiplus.dll", CharSet = CharSet.Unicode)] private static extern int GdipDrawString(nint graphics, string text, int length, nint font, ref FloatRectangle bounds, nint format, nint brush);
    [DllImport("gdiplus.dll")] private static extern int GdipFlush(nint graphics, int intention);
    [DllImport("gdiplus.dll")] private static extern int GdipDisposeImage(nint image);
    [DllImport("gdiplus.dll")] private static extern int GdipDeleteGraphics(nint graphics);
    [DllImport("gdiplus.dll")] private static extern int GdipDeleteFontFamily(nint family);
    [DllImport("gdiplus.dll")] private static extern int GdipDeleteFont(nint font);
    [DllImport("gdiplus.dll")] private static extern int GdipDeleteBrush(nint brush);
    [DllImport("gdiplus.dll")] private static extern int GdipDeleteStringFormat(nint format);
}
