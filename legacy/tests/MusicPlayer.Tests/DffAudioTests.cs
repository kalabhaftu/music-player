using System.Buffers.Binary;
using System.Text;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class DffAudioTests
{
    [Fact]
    public void Uncompressed_duration_and_indexing_read_headers_without_tag_support()
    {
        var folder = Path.Combine(Path.GetTempPath(), "music-player-dff-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        try
        {
            var path = Path.Combine(folder, "duration.dff");
            var bytes = Fixture();
            File.WriteAllBytes(path, bytes);
            File.WriteAllText(Path.ChangeExtension(path, ".lrc"), "DSD lyric");
            Assert.Equal(TimeSpan.FromSeconds(1), DffAudio.ReadDuration(path));
            var track = TrackReader.Read(path, Path.Combine(folder, "artwork"));
            Assert.Equal("duration", track.Title);
            Assert.Equal(TimeSpan.FromSeconds(1), track.Duration);
            Assert.True(track.HasLyrics);
            Assert.Equal(bytes, File.ReadAllBytes(path));
            // A declared payload extending beyond the actual file is rejected.
            File.WriteAllBytes(path, bytes[..^1]);
            Assert.Equal(TimeSpan.Zero, DffAudio.ReadDuration(path));
        }
        finally { Directory.Delete(folder, true); }
    }

    private static byte[] Fixture()
    {
        byte[] Chunk(string id, byte[] data)
        {
            var bytes = new byte[12 + data.Length + (data.Length & 1)];
            Encoding.ASCII.GetBytes(id).CopyTo(bytes, 0);
            BinaryPrimitives.WriteUInt64BigEndian(bytes.AsSpan(4, 8), (ulong)data.Length);
            data.CopyTo(bytes, 12);
            return bytes;
        }
        var rate = new byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(rate, 2822400);
        var properties = Encoding.ASCII.GetBytes("SND ")
            .Concat(Chunk("FS  ", rate))
            .Concat(Chunk("CHNL", [0, 2, .. Encoding.ASCII.GetBytes("SLFTSRGT")]))
            .Concat(Chunk("CMPR", [.. Encoding.ASCII.GetBytes("DSD "), 0])).ToArray();
        var body = Encoding.ASCII.GetBytes("DSD ")
            .Concat(Chunk("FVER", [1, 5, 0, 0]))
            .Concat(Chunk("JUNK", [1, 2, 3]))
            .Concat(Chunk("PROP", properties))
            .Concat(Chunk("DSD ", Enumerable.Repeat((byte)0x69, 2822400 / 8 * 2).ToArray())).ToArray();
        return Chunk("FRM8", body);
    }
}
