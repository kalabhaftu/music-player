using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace MusicPlayer.Core;

public static partial class Lyrics
{
    [GeneratedRegex(@"\[(\d+):(\d{1,2})(?:[.:](\d+))?\]", RegexOptions.CultureInvariant)]
    private static partial Regex TimeStamp();

    [GeneratedRegex(@"^\[([a-z][a-z0-9_-]*):([^\]]*)\]$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex MetadataTag();

    [GeneratedRegex(@"^\[\s*(?:verse|chorus|bridge|intro|outro|hook|pre-chorus|post-chorus|interlude|solo|breakdown|drop|refrain|tag|part)\b[^\]]*\]\s*", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex SectionTag();

    public static string DisplayAt(LyricsDocument document, string plainText, TimeSpan position)
        => document.Lines.Count == 0 ? plainText : document.At(position);

    public static LyricsDocument Parse(string text)
    {
        ArgumentNullException.ThrowIfNull(text);

        var lines = new List<LyricsLine>();
        var metadata = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var offset = TimeSpan.Zero;
        foreach (var raw in NormalizeNewlines(text).Split('\n'))
        {
            var trimmed = raw.Trim();
            if (SectionTag().IsMatch(trimmed)) continue;
            var metadataMatch = MetadataTag().Match(trimmed);
            if (metadataMatch.Success)
            {
                var key = metadataMatch.Groups[1].Value;
                var value = metadataMatch.Groups[2].Value.Trim();
                if (key.Equals("offset", StringComparison.OrdinalIgnoreCase))
                {
                    if (long.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var milliseconds)
                        && milliseconds <= TimeSpan.MaxValue.Ticks / TimeSpan.TicksPerMillisecond
                        && milliseconds >= TimeSpan.MinValue.Ticks / TimeSpan.TicksPerMillisecond)
                    {
                        offset = TimeSpan.FromTicks(milliseconds * TimeSpan.TicksPerMillisecond);
                    }
                }
                else
                {
                    metadata[key] = value;
                }
                continue;
            }

            var timestamps = TimeStamp().Matches(raw);
            if (timestamps.Count == 0) continue;

            for (var start = 0; start < timestamps.Count;)
            {
                var end = start;
                while (end + 1 < timestamps.Count)
                {
                    var between = raw[(timestamps[end].Index + timestamps[end].Length)..timestamps[end + 1].Index];
                    if (!string.IsNullOrWhiteSpace(between)) break;
                    end++;
                }

                var textStart = timestamps[end].Index + timestamps[end].Length;
                var textEnd = end + 1 < timestamps.Count ? timestamps[end + 1].Index : raw.Length;
                var lyric = SectionTag().Replace(raw[textStart..textEnd].Trim(), string.Empty).Trim();
                if (lyric.Length > 0)
                {
                    for (var index = start; index <= end; index++)
                    {
                        if (TryReadTimestamp(timestamps[index], out var time))
                            lines.Add(new(time, lyric));
                    }
                }
                start = end + 1;
            }
        }

        return new(lines.OrderBy(line => line.Time).ToArray(), offset, metadata);
    }

    /// <summary>Returns readable plain lyrics while removing LRC metadata and timestamp markers.</summary>
    public static string PlainText(string text)
    {
        ArgumentNullException.ThrowIfNull(text);
        var output = new List<string>();
        foreach (var raw in NormalizeNewlines(text).Split('\n'))
        {
            var trimmed = raw.Trim();
            if (MetadataTag().IsMatch(trimmed)) continue;
            var lyric = SectionTag().Replace(TimeStamp().Replace(raw, string.Empty).Trim(), string.Empty).Trim();
            if (lyric.Length > 0) output.Add(lyric);
        }
        return string.Join(Environment.NewLine, output);
    }

    public static bool HasUsableContent(string? text)
    {
        if (string.IsNullOrWhiteSpace(text)) return false;
        var document = Parse(text);
        return document.Lines.Any(line => !string.IsNullOrWhiteSpace(line.Text))
            || !string.IsNullOrWhiteSpace(PlainText(text));
    }

    public static bool IsMalformedTimedText(string? text)
    {
        if (string.IsNullOrWhiteSpace(text) || TimeStamp().Matches(text).Count == 0) return false;
        return Parse(text).Lines.Count == 0;
    }

    public static string Format(LyricsDocument document)
    {
        ArgumentNullException.ThrowIfNull(document);
        var output = new StringBuilder();
        if (document.Metadata is not null)
        {
            foreach (var pair in document.Metadata)
            {
                if (pair.Key.Equals("offset", StringComparison.OrdinalIgnoreCase)) continue;
                output.Append('[').Append(pair.Key).Append(':').Append(pair.Value).AppendLine("]");
            }
        }
        if (document.Offset != TimeSpan.Zero)
            output.Append("[offset:").Append((long)document.Offset.TotalMilliseconds).AppendLine("]");

        foreach (var line in document.Lines)
        {
            var time = line.Time < TimeSpan.Zero ? TimeSpan.Zero : line.Time;
            var fraction = (time.Ticks % TimeSpan.TicksPerSecond).ToString("D7", CultureInfo.InvariantCulture).TrimEnd('0');
            if (fraction.Length < 2) fraction = fraction.PadRight(2, '0');
            output.Append('[').Append((long)time.TotalMinutes).Append(':').Append(time.Seconds.ToString("00", CultureInfo.InvariantCulture))
                .Append('.').Append(fraction).Append(']').AppendLine(line.Text);
        }
        return output.ToString();
    }

    private static bool TryReadTimestamp(Match match, out TimeSpan time)
    {
        time = TimeSpan.Zero;
        if (!long.TryParse(match.Groups[1].Value, NumberStyles.None, CultureInfo.InvariantCulture, out var minutes)
            || !int.TryParse(match.Groups[2].Value, NumberStyles.None, CultureInfo.InvariantCulture, out var seconds)
            || seconds is < 0 or >= 60)
        {
            return false;
        }

        var fraction = match.Groups[3].Value;
        var fractionTicksText = fraction.Length > 7 ? fraction[..7] : fraction.PadRight(7, '0');
        var fractionTicks = fractionTicksText.Length == 0
            ? 0
            : long.Parse(fractionTicksText, NumberStyles.None, CultureInfo.InvariantCulture);
        try
        {
            var ticks = checked(minutes * TimeSpan.TicksPerMinute + seconds * TimeSpan.TicksPerSecond + fractionTicks);
            time = TimeSpan.FromTicks(ticks);
            return true;
        }
        catch (OverflowException)
        {
            return false;
        }
    }

    private static string NormalizeNewlines(string text) => text.TrimStart('\uFEFF').Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
}
