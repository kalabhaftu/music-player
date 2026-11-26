using System;
using System.IO;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using MusicPlayer.Desktop.Services;

namespace MusicPlayer.Desktop.Models;

public sealed class UiTrack
{
    public string Path { get; init; } = string.Empty;
    public string Title { get; set; } = string.Empty;
    public string Artist { get; set; } = string.Empty;
    public string Album { get; set; } = string.Empty;
    public string Genre { get; set; } = string.Empty;
    public int Year { get; set; }
    public int TrackNumber { get; set; }
    public TimeSpan Duration { get; set; }
    public string DurationFormatted => Duration.Hours > 0 
        ? Duration.ToString(@"h\:mm\:ss") 
        : Duration.ToString(@"m\:ss");
    public long FileSizeBytes { get; set; }
    public string FileSizeFormatted => $"{FileSizeBytes / (1024.0 * 1024.0):F1} MB";
    public bool IsFavorite { get; set; }
    public string DisplayTrackNumber => TrackNumber > 0 ? TrackNumber.ToString() : "—";
    public string Extension => System.IO.Path.GetExtension(Path).TrimStart('.').ToUpperInvariant();

    public string? ArtworkPath { get; set; }
    public Bitmap? ArtworkBitmap => ArtworkHelper.GetBitmap(ArtworkPath);
    public bool HasArtwork => ArtworkBitmap != null;
    public string Initials { get; set; } = "MP";
    public IBrush CoverBrush { get; set; } = ArtworkHelper.CoverGradient1;
    public string Subtitle => string.IsNullOrWhiteSpace(Album) ? Artist : $"{Artist} · {Album}";
}

public sealed class UiAlbum
{
    public string Title { get; init; } = string.Empty;
    public string Artist { get; init; } = string.Empty;
    public int TrackCount { get; init; }
    public int Year { get; init; }
    public string? ArtworkPath { get; set; }
    public Bitmap? ArtworkBitmap => ArtworkHelper.GetBitmap(ArtworkPath);
    public bool HasArtwork => ArtworkBitmap != null;
    public string Initials { get; set; } = "AL";
    public IBrush CoverBrush { get; set; } = ArtworkHelper.CoverGradient1;
    public string YearFormatted => Year > 0 ? Year.ToString() : "";
    public string TracksFormatted => $"{TrackCount} {(TrackCount == 1 ? "track" : "tracks")}";
}

public sealed class UiArtist
{
    public string Name { get; init; } = string.Empty;
    public int TrackCount { get; init; }
    public int AlbumCount { get; init; }
    public string Initials { get; set; } = "AR";
    public IBrush CoverBrush { get; set; } = ArtworkHelper.CoverGradient1;
    public string SummaryFormatted => $"{TrackCount} {(TrackCount == 1 ? "song" : "songs")} · {AlbumCount} {(AlbumCount == 1 ? "album" : "albums")}";
}

public sealed class UiPlaylist
{
    public string Id { get; init; } = string.Empty;
    public string Name { get; set; } = string.Empty;
    public string Description { get; set; } = string.Empty;
    public int TrackCount { get; set; }
    public bool IsSmart { get; set; }
    public string Initials { get; set; } = "PL";
    public IBrush CoverBrush { get; set; } = ArtworkHelper.CoverGradient1;
    public string CountFormatted => $"{TrackCount} {(TrackCount == 1 ? "song" : "songs")}";
}

public sealed class LyricLine
{
    public double TimeSeconds { get; init; }
    public string Text { get; init; } = string.Empty;
    public bool IsActive { get; set; }
    public bool IsPast { get; set; }
}

public enum NavViewKind
{
    Home,
    Search,
    Favorites,
    Songs,
    Albums,
    Artists,
    Genres,
    RecentlyAdded,
    Playlists,
    Folders,
    Settings,
    Queue,
    Lyrics
}
