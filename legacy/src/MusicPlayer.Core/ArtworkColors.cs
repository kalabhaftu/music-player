namespace MusicPlayer.Core;

public readonly record struct ArtworkColor(byte R, byte G, byte B)
{
    public string Hex => $"#{R:X2}{G:X2}{B:X2}";
    public double Luminance
    {
        get
        {
            static double Linear(byte value) => value / 255d <= .04045 ? value / 255d / 12.92 : Math.Pow((value / 255d + .055) / 1.055, 2.4);
            return .2126 * Linear(R) + .7152 * Linear(G) + .0722 * Linear(B);
        }
    }
}

public sealed record ArtworkPalette(ArtworkColor Background, ArtworkColor Sidebar, ArtworkColor Main,
    ArtworkColor Panel, ArtworkColor Raised, ArtworkColor Hover, ArtworkColor Control, ArtworkColor Accent);

public static class ArtworkColors
{
    /// <summary>Groups cover hues across brightness levels, ignoring transparent pixels and tiny color specks.</summary>
    public static ArtworkPalette FromRgba(ReadOnlySpan<byte> pixels)
    {
        var bins = new (double Weight, double Red, double Green, double Blue)[19];
        double visible = 0;
        for (var i = 0; i + 3 < pixels.Length; i += 4)
        {
            var alpha = pixels[i + 3] / 255d;
            if (alpha < .1) continue;
            var r = pixels[i] / 255d; var g = pixels[i + 1] / 255d; var b = pixels[i + 2] / 255d;
            var max = Math.Max(r, Math.Max(g, b)); var min = Math.Min(r, Math.Min(g, b)); var chroma = max - min;
            var saturation = max == 0 ? 0 : chroma / max;
            var hue = chroma == 0 ? 0 : max == r ? ((g - b) / chroma + 6) % 6 : max == g ? (b - r) / chroma + 2 : (r - g) / chroma + 4;
            var binIndex = saturation >= .18 && max >= .08 ? (int)Math.Floor(hue * 3) % 18 : 18;
            ref var bin = ref bins[binIndex];
            // Counting related shades together avoids favoring a flat background merely because it has fewer RGB variations.
            bin.Weight += alpha; bin.Red += pixels[i] * alpha; bin.Green += pixels[i + 1] * alpha; bin.Blue += pixels[i + 2] * alpha;
            visible += alpha;
        }
        var selected = 18;
        for (var i = 0; i < 18; i++)
            if (bins[i].Weight >= visible * .05 && (selected == 18 || bins[i].Weight > bins[selected].Weight)) selected = i;
        var chosen = bins[selected];
        static byte Byte(double value) => (byte)Math.Clamp(Math.Round(value), 0, 255);
        var seed = chosen.Weight > 0 ? new ArtworkColor(Byte(chosen.Red / chosen.Weight), Byte(chosen.Green / chosen.Weight), Byte(chosen.Blue / chosen.Weight)) : new ArtworkColor(128, 128, 128);
        // Neutral artwork stays neutral; the mean of competing red/blue hues cannot invent a purple surface.
        if (selected == 18)
        {
            var gray = Byte(.2126 * seed.R + .7152 * seed.G + .0722 * seed.B);
            seed = new(gray, gray, gray);
        }
        var maximum = Math.Max(1, (int)Math.Max(seed.R, Math.Max(seed.G, seed.B)));
        ArtworkColor Tone(double shade) => new(Byte(10 + seed.R / (double)maximum * shade), Byte(10 + seed.G / (double)maximum * shade), Byte(10 + seed.B / (double)maximum * shade));
        var background = Tone(38); var main = Tone(42); var raised = Tone(50); var hover = Tone(58); var control = Tone(64);
        ArtworkColor Lighten(double amount) => new(Byte(seed.R + (255 - seed.R) * amount), Byte(seed.G + (255 - seed.G) * amount), Byte(seed.B + (255 - seed.B) * amount));
        var accent = seed;
        // Use actual sRGB relative luminance and ensure readable accent text on every generated surface.
        for (var step = 0; step <= 100 && (accent.Luminance + .05) / (control.Luminance + .05) < 4.5; step++) accent = Lighten(step / 100d);
        return new(background, Tone(34), main, background, raised, hover, control, accent);
    }
}
