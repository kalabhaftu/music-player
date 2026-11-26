using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiPlaylistCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "createPlaylist":
            {
                var id = OptionalTrack(payload, "trackId");
                var playlist = _playlistLibrary.Create(String(payload, "name"), id?.Path);
                PublishLibraryChanged();
                return new { playlist = _libraryQueries.PlaylistDto(_store.GetPlaylistSummaries().First(item => item.Id == playlist.Id)) };
            }
            case "renamePlaylist":
            {
                var id = PlaylistId(payload); _playlistLibrary.Rename(id, String(payload, "name"));
                PublishLibraryChanged(); return null;
            }
            case "deletePlaylist":
                _playlistLibrary.Delete(PlaylistId(payload)); PublishLibraryChanged(); return null;
            case "playPlaylist":
            case "shufflePlaylist":
            {
                var id = PlaylistId(payload); var paths = _store.GetPlaylistPaths(id);
                if (!_playbackQueue.StartView(paths, name == "shufflePlaylist")) return null;
                _libraryQueries.SetPlaylistContext(id);
                if (_store.GetTrack(_queue[0]) is { } first) PlayTrack(first, false, 0);
                PublishQueueState(force: true); return null;
            }
            case "exportPlaylist": await ExportPlaylistAsync(PlaylistId(payload)); return null;
            case "importPlaylist": await ImportPlaylistAsync(); return null;
            case "addToPlaylist":
                _playlistLibrary.AddTrack(PlaylistId(payload), RequireTrack(payload, "trackId").Path);
                PublishLibraryChanged(); return null;
            case "removePlaylistTrack":
                _playlistLibrary.RemoveTrackOccurrence(PlaylistId(payload), Int(payload, "position", -1));
                PublishLibraryChanged(); return null;
            default: throw new InvalidOperationException("This Music Player command is not available in the playlist handler.");
        }
    }
}
