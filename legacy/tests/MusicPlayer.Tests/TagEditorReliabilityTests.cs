using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class TagEditorReliabilityTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-tag-tests-" + Guid.NewGuid().ToString("N"));

    [Fact]
    public async Task Wave_filename_alias_can_be_indexed_edited_and_restored()
    {
        Directory.CreateDirectory(_root);
        var path = Path.Combine(_root, "alias.WAVE");
        WriteLargeWave(path, 44100 * 4);
        var original = File.ReadAllBytes(path);
        var editor = new TagEditor(Path.Combine(_root, "backups"));
        var backup = await editor.SaveAsync(path, new TagEdit(Title: "Wave alias", Artist: "Artist", Lyrics: "Embedded lyric"));
        var track = TrackReader.Read(path, Path.Combine(_root, "artwork"));
        Assert.Equal("Wave alias", track.Title);
        Assert.Equal("Artist", track.Artist);
        Assert.True(track.HasLyrics);
        Assert.Equal("Embedded lyric", LyricsFiles.ReadRaw(path));
        Assert.Equal(TimeSpan.FromSeconds(1), track.Duration);
        Assert.Equal(track.Duration, TrackInformation.Read(path).Duration);
        Assert.NotNull(TagEditor.ReadAdditionalStandardFields(path));
        await editor.RestoreAsync(backup);
        Assert.Equal(original, File.ReadAllBytes(path));
    }

    [Fact]
    public async Task Save_async_keeps_only_five_recovery_copies_per_track()
    {
        var track = Path.Combine(_root, "retention.wav");
        var backupFolder = Path.Combine(_root, "backups");
        Directory.CreateDirectory(_root);
        WriteWave(track);
        var editor = new TagEditor(backupFolder);
        var saved = new List<TagBackup>();

        for (var i = 0; i < 7; i++)
        {
            saved.Add(await editor.SaveAsync(track, new TagEdit(Title: $"Title {i}")));
            await Task.Delay(2);
        }

        var remaining = editor.ListBackups(track);
        Assert.Equal(5, remaining.Count);
        Assert.All(remaining, backup => Assert.True(File.Exists(backup.BackupPath)));
        Assert.DoesNotContain(remaining, backup => backup.BackupPath == saved[0].BackupPath);
        Assert.DoesNotContain(remaining, backup => backup.BackupPath == saved[1].BackupPath);
        Assert.Equal(5, Directory.GetFiles(backupFolder, "*.bak").Length);
    }

    [Fact]
    public async Task Restore_async_saves_the_current_version_as_an_undo_snapshot()
    {
        var track = Path.Combine(_root, "undo.wav");
        Directory.CreateDirectory(_root);
        WriteWave(track);
        var editor = new TagEditor(Path.Combine(_root, "backups"));
        var originalBytes = File.ReadAllBytes(track);
        var firstEdit = await editor.SaveAsync(track, new TagEdit(Title: "First edit"));
        await editor.SaveAsync(track, new TagEdit(Title: "Second edit"));
        var editedBytes = File.ReadAllBytes(track);

        var undo = await editor.RestoreAsync(firstEdit);

        Assert.Equal(originalBytes, File.ReadAllBytes(track));
        Assert.True(File.Exists(undo.BackupPath));
        Assert.Equal(editedBytes, File.ReadAllBytes(undo.BackupPath));
        using var media = TagLib.File.Create(track);
        Assert.Null(media.Tag.Title);
        Assert.Equal(3, editor.ListBackups(track).Count);
    }

    [Fact]
    public async Task Cancelling_a_large_tag_save_leaves_the_original_and_removes_partial_files()
    {
        var track = Path.Combine(_root, "large.wav");
        var backupFolder = Path.Combine(_root, "backups");
        Directory.CreateDirectory(_root);
        WriteLargeWave(track, 16 * 1024 * 1024);
        var original = File.ReadAllBytes(track);
        using var cancellation = new CancellationTokenSource();
        var progress = new InlineProgress(value =>
        {
            if (value.Phase == "Preparing edit" && value.BytesCopied >= 4 * 1024 * 1024) cancellation.Cancel();
        });

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            new TagEditor(backupFolder).SaveAsync(track, new TagEdit(Title: "Must not commit"), progress, cancellation.Token));

        Assert.Equal(original, File.ReadAllBytes(track));
        Assert.Empty(Directory.GetFiles(_root, ".musicplayer-stage-*"));
        Assert.Empty(Directory.GetFiles(backupFolder, "*.partial"));
    }

    private static void WriteWave(string path) => WriteLargeWave(path, 4);

    private static void WriteLargeWave(string path, int dataLength)
    {
        const int sampleRate = 44100, channels = 2, bits = 16;
        using var writer = new BinaryWriter(File.Create(path));
        writer.Write(System.Text.Encoding.ASCII.GetBytes("RIFF")); writer.Write(36 + dataLength); writer.Write(System.Text.Encoding.ASCII.GetBytes("WAVE"));
        writer.Write(System.Text.Encoding.ASCII.GetBytes("fmt ")); writer.Write(16); writer.Write((short)1); writer.Write((short)channels);
        writer.Write(sampleRate); writer.Write(sampleRate * channels * bits / 8); writer.Write((short)(channels * bits / 8)); writer.Write((short)bits);
        writer.Write(System.Text.Encoding.ASCII.GetBytes("data")); writer.Write(dataLength); writer.Write(new byte[dataLength]);
    }

    private sealed class InlineProgress(Action<TagEditProgress> report) : IProgress<TagEditProgress>
    {
        public void Report(TagEditProgress value) => report(value);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, true);
    }
}
