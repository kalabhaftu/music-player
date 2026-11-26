namespace MusicPlayer.Core;

internal static class AudioTags
{
    // .wave is a WAV filename alias that TagLib's extension registry omits.
    internal static TagLib.File Open(string path) =>
        Path.GetExtension(path).Equals(".wave", StringComparison.OrdinalIgnoreCase)
            ? TagLib.File.Create(path, "taglib/wav", TagLib.ReadStyle.Average)
            : TagLib.File.Create(path);
}
