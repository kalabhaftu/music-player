using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Playback.Tests;

public sealed class FormatTagTests
{
    public static IEnumerable<object[]> Formats => LibraryScanner.AudioExtensions.Order().Select(extension => new object[] { extension });

    [Theory]
    [MemberData(nameof(Formats))]
    public async Task Tag_edit_preserves_other_fields_playable_content_and_recovery_bytes(string extension)
    {
        var fixtures = Environment.GetEnvironmentVariable("MUSICPLAYER_MEDIA_FIXTURES")
            ?? throw new InvalidOperationException("Generate genuine codec fixtures before format validation.");
        var folder = Path.Combine(Path.GetTempPath(), "music-player-format-tags-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        try
        {
            var path = Path.Combine(folder, "editable" + extension);
            File.Copy(Path.Combine(fixtures, "fixture" + extension), path);
            var original = File.ReadAllBytes(path);
            var editor = new TagEditor(Path.Combine(folder, "backups"));
            // TagLib# 2.3 cannot edit DFF or TTA. The safe rejection must leave
            // their media bytes intact; playback/indexing support is independent.
            if (extension is ".dff" or ".tta")
            {
                await Assert.ThrowsAsync<TagLib.UnsupportedFormatException>(() => editor.SaveAsync(path, new TagEdit(Title: "Unchanged")));
                Assert.Equal(original, File.ReadAllBytes(path));
                return;
            }
            string? artist;
            string? album;
            TimeSpan duration;
            using (var media = OpenFixture(path, extension))
            {
                artist = media.Tag.FirstPerformer;
                album = media.Tag.Album;
                duration = media.Properties.Duration;
            }
            var backup = await editor.SaveAsync(path, new TagEdit(Title: "Stable release tag check"));
            using (var edited = OpenFixture(path, extension))
            {
                Assert.Equal("Stable release tag check", edited.Tag.Title);
                Assert.Equal(artist, edited.Tag.FirstPerformer);
                Assert.Equal(album, edited.Tag.Album);
                Assert.InRange((edited.Properties.Duration - duration).TotalMilliseconds, -250, 250);
            }
            Assert.Equal(original, File.ReadAllBytes(backup.BackupPath));
            await editor.RestoreAsync(backup);
            Assert.Equal(original, File.ReadAllBytes(path));
        }
        finally { Directory.Delete(folder, recursive: true); }
    }

    private static TagLib.File OpenFixture(string path, string extension) => extension == ".wave"
        ? TagLib.File.Create(path, "taglib/wav", TagLib.ReadStyle.Average)
        : TagLib.File.Create(path);
}
