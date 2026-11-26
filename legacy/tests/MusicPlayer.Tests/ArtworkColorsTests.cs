using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class ArtworkColorsTests
{
    [Fact]
    public void CompetingHuesDoNotInventAnAverageColor()
    {
        var palette = ArtworkColors.FromRgba(Pixels((60, 200, 15, 15, 255), (40, 15, 15, 200, 255)));
        Assert.True(palette.Main.R > palette.Main.B * 2);
        Assert.True(palette.Accent.R > palette.Accent.B);
        Assert.True((palette.Accent.Luminance + .05) / (palette.Control.Luminance + .05) >= 4.5);
    }

    [Fact]
    public void TransparentPixelsAndTinyBrightMarksCannotSelectTheHue()
    {
        var palette = ArtworkColors.FromRgba(Pixels((90, 10, 90, 40, 255), (3, 255, 0, 0, 255), (500, 0, 0, 255, 0)));
        Assert.True(palette.Main.G > palette.Main.R);
        Assert.True(palette.Main.G > palette.Main.B);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(128)]
    [InlineData(255)]
    public void GrayscaleRemainsNeutralAndReadable(byte value)
    {
        var palette = ArtworkColors.FromRgba(Pixels((100, value, value, value, 255)));
        Assert.Equal(palette.Main.R, palette.Main.G);
        Assert.Equal(palette.Main.G, palette.Main.B);
        Assert.Equal(palette.Accent.R, palette.Accent.B);
        Assert.True((palette.Accent.Luminance + .05) / (palette.Control.Luminance + .05) >= 4.5);
    }

    [Fact]
    public void EmptyOrTransparentCoverHasReadableNeutralFallback()
    {
        Assert.Equal(ArtworkColors.FromRgba([]), ArtworkColors.FromRgba(Pixels((100, 255, 0, 0, 0))));
    }

    private static byte[] Pixels(params (int Count, byte R, byte G, byte B, byte A)[] groups)
        => groups.SelectMany(group => Enumerable.Range(0, group.Count).SelectMany(_ => new[] { group.R, group.G, group.B, group.A })).ToArray();
}
