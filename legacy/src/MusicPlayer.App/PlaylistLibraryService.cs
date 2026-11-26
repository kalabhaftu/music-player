using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>
/// Owns playlist mutations against the library store without depending on the window or UI.
/// </summary>
internal sealed class PlaylistLibraryService(LibraryStore store)
{
    public Playlist Create(string name, string? initialTrackPath) =>
        store.CreatePlaylist(name, initialTrackPath is null ? null : [initialTrackPath]);

    public void Rename(string playlistId, string name) => store.RenamePlaylist(playlistId, name);

    public void Delete(string playlistId) => store.DeletePlaylist(playlistId);

    public void AddTrack(string playlistId, string trackPath) => store.AddToPlaylist(playlistId, [trackPath]);

    public void RemoveTrackOccurrence(string playlistId, int position) => store.RemoveFromPlaylist(playlistId, position);
}
