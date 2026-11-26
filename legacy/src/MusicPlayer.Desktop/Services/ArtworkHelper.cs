using System;
using System.Collections.Concurrent;
using System.IO;
using System.Text.RegularExpressions;
using Avalonia.Media;
using Avalonia.Media.Imaging;

namespace MusicPlayer.Desktop.Services;

public static partial class ArtworkHelper
{
    private static readonly ConcurrentDictionary<string, Bitmap> ImageCache = new(StringComparer.OrdinalIgnoreCase);

    // 6 Subtle gradients matching components.css .cover.c1 through .cover.c6
    public static readonly IBrush CoverGradient1 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#1B1D26"), 0.0),
            new GradientStop(Color.Parse("#2A2F3F"), 0.58),
            new GradientStop(Color.Parse("#3B4358"), 1.0)
        ]
    };

    public static readonly IBrush CoverGradient2 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#162124"), 0.0),
            new GradientStop(Color.Parse("#213337"), 0.58),
            new GradientStop(Color.Parse("#30494E"), 1.0)
        ]
    };

    public static readonly IBrush CoverGradient3 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#251B1D"), 0.0),
            new GradientStop(Color.Parse("#38272B"), 0.58),
            new GradientStop(Color.Parse("#4E373C"), 1.0)
        ]
    };

    public static readonly IBrush CoverGradient4 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#172019"), 0.0),
            new GradientStop(Color.Parse("#243328"), 0.58),
            new GradientStop(Color.Parse("#364A3B"), 1.0)
        ]
    };

    public static readonly IBrush CoverGradient5 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#231F18"), 0.0),
            new GradientStop(Color.Parse("#353025"), 0.58),
            new GradientStop(Color.Parse("#4B4432"), 1.0)
        ]
    };

    public static readonly IBrush CoverGradient6 = new LinearGradientBrush
    {
        StartPoint = new Avalonia.RelativePoint(0, 0, Avalonia.RelativeUnit.Relative),
        EndPoint = new Avalonia.RelativePoint(1, 1, Avalonia.RelativeUnit.Relative),
        GradientStops =
        [
            new GradientStop(Color.Parse("#1B1B20"), 0.0),
            new GradientStop(Color.Parse("#2A2A32"), 0.58),
            new GradientStop(Color.Parse("#3D3D48"), 1.0)
        ]
    };

    private static readonly IBrush[] Gradients =
    [
        CoverGradient1, CoverGradient2, CoverGradient3, CoverGradient4, CoverGradient5, CoverGradient6
    ];

    public static IBrush GetCoverBrush(string? seed)
    {
        if (string.IsNullOrWhiteSpace(seed)) return CoverGradient1;
        uint hash = 2166136261;
        foreach (char c in seed)
        {
            hash = (hash ^ c) * 16777619;
        }
        int index = (int)(hash % (uint)Gradients.Length);
        return Gradients[index];
    }

    [GeneratedRegex(@"\([^)]*\)|\[[^\]]*\]|\{[^}]*\}")]
    private static partial Regex ParenthesesRegex();

    [GeneratedRegex(@"[^\p{L}\p{N}\s]")]
    private static partial Regex NonAlphanumericRegex();

    public static string GetInitials(string? text, string? fallback = "MP")
    {
        if (string.IsNullOrWhiteSpace(text)) return fallback ?? "MP";

        string clean = ParenthesesRegex().Replace(text, " ");
        clean = NonAlphanumericRegex().Replace(clean, " ").Trim();

        var words = clean.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0)
        {
            string alphaOnly = NonAlphanumericRegex().Replace(text, "");
            return alphaOnly.Length >= 2 ? alphaOnly[..2].ToUpperInvariant() : fallback ?? "MP";
        }

        if (words.Length == 1)
        {
            return words[0].Length >= 2 ? words[0][..2].ToUpperInvariant() : words[0].ToUpperInvariant();
        }

        return (words[0][0].ToString() + words[1][0].ToString()).ToUpperInvariant();
    }

    public static Bitmap? GetBitmap(string? path)
    {
        if (string.IsNullOrEmpty(path) || !File.Exists(path)) return null;

        try
        {
            return ImageCache.GetOrAdd(path, p => new Bitmap(p));
        }
        catch
        {
            return null;
        }
    }
}
