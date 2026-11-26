using System.Buffers.Binary;

namespace MusicPlayer.Core;

/// <summary>Reads the duration of uncompressed DSDIFF without loading its audio payload.</summary>
public static class DffAudio
{
    public static TimeSpan ReadDuration(string path)
    {
        using var file = File.OpenRead(path);
        Span<byte> header = stackalloc byte[16];
        if (file.Read(header) != header.Length || !header[..4].SequenceEqual("FRM8"u8) || !header[12..].SequenceEqual("DSD "u8))
            return TimeSpan.Zero;
        uint rate = 0;
        ushort channels = 0;
        bool uncompressed = false;
        long propertiesEnd = 0;
        // DSDIFF properties precede audio. Bound traversal of malformed headers;
        // skip unknown chunks using their checked lengths, never their payloads.
        for (var chunks = 0; chunks < 256 && file.Position < file.Length; chunks++)
        {
            if (file.Read(header[..12]) != 12) return TimeSpan.Zero;
            var bytes = BinaryPrimitives.ReadUInt64BigEndian(header[4..12]);
            if (bytes > (ulong)(file.Length - file.Position)) return TimeSpan.Zero;
            var end = file.Position + (long)bytes;
            if (propertiesEnd > 0 && file.Position <= propertiesEnd && end > propertiesEnd) return TimeSpan.Zero;
            if (header[..4].SequenceEqual("PROP"u8) && bytes >= 4)
            {
                if (file.Read(header[..4]) != 4 || !header[..4].SequenceEqual("SND "u8)) return TimeSpan.Zero;
                propertiesEnd = end;
                continue;
            }
            if (header[..4].SequenceEqual("FS  "u8) && bytes == 4)
            {
                file.ReadExactly(header[..4]);
                rate = BinaryPrimitives.ReadUInt32BigEndian(header[..4]);
            }
            else if (header[..4].SequenceEqual("CHNL"u8) && bytes >= 2)
            {
                file.ReadExactly(header[..2]);
                channels = BinaryPrimitives.ReadUInt16BigEndian(header[..2]);
            }
            else if (header[..4].SequenceEqual("CMPR"u8) && bytes >= 4)
            {
                file.ReadExactly(header[..4]);
                uncompressed = header[..4].SequenceEqual("DSD "u8);
            }
            else if (header[..4].SequenceEqual("DSD "u8))
            {
                if (!uncompressed || rate == 0 || channels == 0) return TimeSpan.Zero;
                var ticks = bytes * (8d * TimeSpan.TicksPerSecond) / rate / channels;
                return ticks < long.MaxValue ? TimeSpan.FromTicks((long)ticks) : TimeSpan.Zero;
            }
            file.Position = end + (long)(bytes & 1);
            if (propertiesEnd > 0 && file.Position >= propertiesEnd) propertiesEnd = 0;
        }
        return TimeSpan.Zero;
    }
}
