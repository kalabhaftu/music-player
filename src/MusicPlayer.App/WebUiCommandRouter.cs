using System.Text.Json;

namespace MusicPlayer.App;

/// <summary>
/// Owns the explicit command contract exposed to the Web UI and routes each command
/// to the existing feature-specific handler. This keeps bridge validation and
/// dispatch on one allowlist without moving feature behavior into the window shell.
/// </summary>
internal sealed class WebUiCommandRouter
{
    private readonly Dictionary<string, Func<JsonElement, Task<object?>>> _routes = new(StringComparer.Ordinal);

    public WebUiCommandRouter(
        Func<string, JsonElement, Task<object?>> data,
        Func<string, JsonElement, Task<object?>> playback,
        Func<string, JsonElement, Task<object?>> playlists,
        Func<string, JsonElement, Task<object?>> folders,
        Func<string, JsonElement, Task<object?>> metadata,
        Func<string, JsonElement, Task<object?>> settings,
        Func<string, JsonElement, Task<object?>> utilities)
    {
        Register(data,
            "getBootstrap", "uiReady", "getAbout", "getReleaseHistory", "getHome", "beginSearch", "search", "getTracks", "getGroups", "getFolders", "getArtistAlbums",
            "getPlaylists", "getPlaylistTracks", "getQueue", "getCurrentTrack", "getLyrics", "getAudioSettings",
            "getSettings", "getDuplicates", "getDuplicateFiles", "getTrackDetails", "getTags", "getExclusions", "setView");
        Register(playback,
            "playTrack", "playView", "shuffleView", "playGroup", "playPause", "previous", "next", "toggleShuffle",
            "cycleRepeat", "toggleAbRepeat", "seek", "setVolume", "toggleFavorite", "setRating", "playNext",
            "addToQueue", "moveQueue", "removeQueue", "clearQueue", "playQueueEntry");
        Register(playlists,
            "createPlaylist", "renamePlaylist", "deletePlaylist", "playPlaylist", "shufflePlaylist", "exportPlaylist",
            "importPlaylist", "addToPlaylist", "removePlaylistTrack");
        Register(folders,
            "addFolder", "removeRoot", "scanLibrary", "rebuildLibraryIndex", "toggleScanPause", "cancelScan", "showInFolder", "showFilePath");
        Register(metadata, "saveLyrics", "searchLyrics", "saveTags", "restoreTags", "pickArtwork");
        Register(settings,
            "setAudioDevice", "setCrossfade", "refreshAudioDevices", "setEqualizerPreset", "setEqualizerBand", "setEqualizerBands", "setExtensionEnabled",
            "saveEqualizerPreset", "updateSettings", "resetUiSettings", "addExclusion", "removeExclusion", "checkUpdates");
    Register(utilities, "openLogs", "exportLogs", "openRelease", "openLegalFile", "setPanelMode", "setImmersiveMode", "openDefaultApps", "copyTrackPath", "reportFrontendError");
    }

    public bool CanRoute(string name) => _routes.ContainsKey(name);

    public Task<object?> RouteAsync(string name, JsonElement payload)
    {
        if (!_routes.TryGetValue(name, out var handler))
            throw new InvalidOperationException("This Music Player command is not available.");

        return handler(payload);
    }

    private void Register(Func<string, JsonElement, Task<object?>> handler, params string[] names)
    {
        foreach (var name in names)
        {
            if (!_routes.TryAdd(name, payload => handler(name, payload)))
                throw new InvalidOperationException($"The Web UI command '{name}' is registered more than once.");
        }
    }
}
