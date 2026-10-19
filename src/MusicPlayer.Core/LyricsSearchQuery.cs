using System.Text.RegularExpressions;

namespace MusicPlayer.Core;

/// <summary>Removes upload/version decorations without discarding the song's identity.</summary>
public static partial class LyricsSearchQuery
{
    [GeneratedRegex(@"\b(?:remix|reverb(?:ed)?|slowed|sped[\s-]*up|speed[\s-]*up|nightcore|bass[\s-]*boosted|8d|official|lyric(?:s)?|audio|music\s+video|visuali[sz]er|hd|hq|\d{2,4}\s*kbps?|\d{2,4}k)\b", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex Decoration();
    [GeneratedRegex(@"\([^)]*\)|\[[^\]]*\]|\{[^}]*\}", RegexOptions.CultureInvariant)]
    private static partial Regex Bracketed();
    [GeneratedRegex(@"\s+", RegexOptions.CultureInvariant)]
    private static partial Regex Whitespace();

    public static (string Title, string Artist) Normalize(string title, string artist)
    {
        title = title.Trim();
        artist = artist.Trim();
        if (artist.Length == 0 || artist.Equals("Unknown artist", StringComparison.OrdinalIgnoreCase) || artist.Equals("Unknown", StringComparison.OrdinalIgnoreCase))
        {
            artist = "";
            var separator = title.IndexOf(" - ", StringComparison.Ordinal);
            if (separator > 0 && separator < title.Length - 3)
            {
                artist = title[..separator].Trim();
                title = title[(separator + 3)..].Trim();
            }
        }
        else if (title.StartsWith(artist + " - ", StringComparison.OrdinalIgnoreCase)) title = title[(artist.Length + 3)..];
        title = Bracketed().Replace(title, match => Decoration().IsMatch(match.Value) ? " " : match.Value);
        // These suffixes are common on local downloads, with or without brackets.
        title = Regex.Replace(title, @"(?:\s*[-–—|+]\s*|\s+)(?:slowed|reverb(?:ed)?|sped[\s-]*up|speed[\s-]*up|nightcore|remix|official\s+(?:audio|music\s+video|video)|lyrics?\s+video)\b.*$", "", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        return (Whitespace().Replace(title, " ").Trim(' ', '-', '–', '—', '|', '+'), artist);
    }
}
