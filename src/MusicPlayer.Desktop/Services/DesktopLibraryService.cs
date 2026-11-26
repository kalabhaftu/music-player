using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using MusicPlayer.Core;
using MusicPlayer.Desktop.Models;

namespace MusicPlayer.Desktop.Services;

public sealed class DesktopLibraryService
{
    private readonly LibraryStore _store;

    public DesktopLibraryService()
    {
        _store = LibraryStore.InAppData();
    }

    public LibraryStore Store => _store;

    public IReadOnlyList<UiTrack> GetAllTracks()
    {
        return _store.GetTracks().Select(MapTrack).ToList();
    }

    public IReadOnlyList<UiTrack> GetFavoriteTracks()
    {
        return _store.GetTracks(filter: "favorites").Select(MapTrack).ToList();
    }

    public IReadOnlyList<UiTrack> GetRecentlyAddedTracks(int limit = 50)
    {
        return _store.GetTracks(sort: TrackSort.Added, descending: true).Take(limit).Select(MapTrack).ToList();
    }

    public IReadOnlyList<UiAlbum> GetAlbums()
    {
        var groups = _store.GetGroupsPage("album", offset: 0, pageSize: 200);
        return groups.Select(a => new UiAlbum
        {
            Title = a.Name,
            Artist = a.Artist ?? "Unknown Artist",
            TrackCount = a.TrackCount,
            Year = (int)a.Year,
            ArtworkPath = a.ArtworkPath,
            Initials = ArtworkHelper.GetInitials(a.Name, "AL"),
            CoverBrush = ArtworkHelper.GetCoverBrush(a.Name)
        }).ToList();
    }

    public IReadOnlyList<UiArtist> GetArtists()
    {
        var groups = _store.GetGroupsPage("artist", offset: 0, pageSize: 200);
        return groups.Select(a => new UiArtist
        {
            Name = a.Name,
            TrackCount = a.TrackCount,
            AlbumCount = 0,
            Initials = ArtworkHelper.GetInitials(a.Name, "AR"),
            CoverBrush = ArtworkHelper.GetCoverBrush(a.Name)
        }).ToList();
    }

    public IReadOnlyList<UiPlaylist> GetPlaylists()
    {
        var playlists = _store.GetPlaylistSummaries();
        return playlists.Select(p => new UiPlaylist
        {
            Id = p.Id,
            Name = p.Name,
            TrackCount = p.TrackCount,
            Initials = ArtworkHelper.GetInitials(p.Name, "PL"),
            CoverBrush = ArtworkHelper.GetCoverBrush(p.Name)
        }).ToList();
    }

    public IReadOnlyList<UiPlaylist> GetSmartPlaylists()
    {
        return
        [
            new UiPlaylist
            {
                Id = "smart-mp",
                Name = "Most Played",
                Description = "Songs played most often",
                IsSmart = true,
                Initials = "MP",
                CoverBrush = ArtworkHelper.CoverGradient1
            },
            new UiPlaylist
            {
                Id = "smart-wl",
                Name = "With Lyrics",
                Description = "Songs with available lyrics",
                IsSmart = true,
                Initials = "WL",
                CoverBrush = ArtworkHelper.CoverGradient4
            },
            new UiPlaylist
            {
                Id = "smart-rp",
                Name = "Recently Played",
                Description = "Your recent listening history",
                IsSmart = true,
                Initials = "RP",
                CoverBrush = ArtworkHelper.CoverGradient5
            }
        ];
    }

    public IReadOnlyList<string> GetLibraryFolders()
    {
        var json = _store.GetSetting("library-roots");
        if (string.IsNullOrWhiteSpace(json)) return Array.Empty<string>();
        try { return JsonSerializer.Deserialize<string[]>(json) ?? Array.Empty<string>(); }
        catch { return Array.Empty<string>(); }
    }

    public void AddLibraryFolder(string path)
    {
        var roots = GetLibraryFolders().ToList();
        if (!roots.Any(r => string.Equals(r, path, StringComparison.OrdinalIgnoreCase)))
        {
            roots.Add(path);
            _store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
        }
    }

    public void RemoveLibraryFolder(string path)
    {
        var roots = GetLibraryFolders().Where(r => !string.Equals(r, path, StringComparison.OrdinalIgnoreCase)).ToList();
        _store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
    }

    public void SetFavorite(string path, bool isFavorite)
    {
        _store.SetFavorite(path, isFavorite);
    }

    public IReadOnlyList<UiTrack> GetTracksForAlbum(string album, string artist)
    {
        return _store.GetTracks(groupColumn: "album", groupValue: album).Select(MapTrack).ToList();
    }

    public IReadOnlyList<UiTrack> GetTracksForArtist(string artist)
    {
        return _store.GetTracks(groupColumn: "artist", groupValue: artist).Select(MapTrack).ToList();
    }

    public IReadOnlyList<UiTrack> Search(string query)
    {
        if (string.IsNullOrWhiteSpace(query))
            return GetAllTracks();

        return _store.GetTracks(search: query.Trim()).Select(MapTrack).ToList();
    }

    public static UiTrack MapTrack(Track t)
    {
        var albumOrArtist = !string.IsNullOrWhiteSpace(t.Album) ? t.Album : t.Artist;
        return new UiTrack
        {
            Path = t.Path,
            Title = string.IsNullOrWhiteSpace(t.Title) ? Path.GetFileNameWithoutExtension(t.Path) : t.Title,
            Artist = string.IsNullOrWhiteSpace(t.Artist) ? "Unknown Artist" : t.Artist,
            Album = string.IsNullOrWhiteSpace(t.Album) ? "Unknown Album" : t.Album,
            Genre = t.Genre ?? "",
            Year = (int)t.Year,
            TrackNumber = (int)t.TrackNumber,
            Duration = t.Duration,
            FileSizeBytes = t.FileSize,
            IsFavorite = t.Favorite,
            ArtworkPath = t.ArtworkPath,
            Initials = ArtworkHelper.GetInitials(t.Title, "MP"),
            CoverBrush = ArtworkHelper.GetCoverBrush(albumOrArtist ?? t.Title)
        };
    }
}
