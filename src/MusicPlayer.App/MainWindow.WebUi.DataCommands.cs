using System.Text.Json;
using System.Reflection;
using System.Text.RegularExpressions;
using MusicPlayer.Core;
using Microsoft.UI.Xaml;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiDataCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "uiReady":
                if (!_startupBackgroundWorkStarted && !_windowClosed)
                {
                    _startupBackgroundWorkStarted = true;
                    StartStartupScan();
                    _ = CheckForUpdatesAsync(false);
                    _ = PrepareDuplicatesInBackgroundAsync();
                }
                return null;
            case "getBootstrap":
            {
                _webUiBootstrapped = true;
                var currentTrack = _playback.CurrentTrack;
                var settingState = WebSettings();
                return new
                {
                    view = _libraryQueries.CurrentView, search = _libraryQueries.CurrentSearch,
                    group = _libraryQueries.CurrentGroup, playlist = _libraryQueries.CurrentPlaylist,
                    videoSupportEnabled = _libraryLocations.GetEnabledVideoExtensions().Length > 0,
                    track = currentTrack is null ? null : _libraryQueries.TrackDto(currentTrack, includePath: true),
                    playing = _playback.IsPlaying, positionSeconds = _playback.Position / 1000d,
                    durationSeconds = _playback.Duration / 1000d, volume = _playback.Volume, isVideo = _playback.IsVideoMode,
                    shuffle = _shuffle, repeat = _repeatMode,
                    repeatA = _repeatA?.TotalSeconds, repeatB = _repeatB?.TotalSeconds,
                    queue = QueueDtos(Math.Max(0, _queueIndex), 30), queueOffset = Math.Max(0, _queueIndex), queueTotal = _queue.Count, queueIndex = _queueIndex,
                    panel = _store.GetSetting("right-sidebar-mode") == "Info" ? "info" : "queue",
                    version = ApplicationVersion.Resolve(typeof(MainWindow).Assembly), libraryRevision = _libraryRevision,
                    settings = settingState, resolvedTheme = ShellRoot.ActualTheme == ElementTheme.Light ? "Light" : "Dark", scan = ScanDto(),
                    updateCheckActive = Volatile.Read(ref _updateCheckActive) != 0,
                    updateAvailable = _webAvailableRelease is { } available ? new { tag = available.Tag, url = available.Url } : null,
                    latestRelease = GitHubUpdates.ReadCachedRelease(_store.GetSetting("update-release-tag"), _store.GetSetting("update-release-url")) is { } latest
                        ? new { tag = latest.Tag, url = latest.Url } : null
                };
            }
            case "getAbout": return await Task.Run(AboutInfo);
            case "getReleaseHistory": return (await GitHubUpdates.GetReleaseHistoryAsync()).Select(release => new { tag = release.Tag, url = release.Url, prerelease = release.Prerelease }).ToArray();
            case "getHome": return await Task.Run(() => _libraryQueries.HomeData(Int(payload, "pageSize", 8)));
            case "beginSearch": _libraryQueries.BeginSearch(Long(payload, "requestId")); return null;
            case "search": return await _libraryQueries.SearchAsync(payload);
            case "getTracks":
            {
                // Persist view state on the UI dispatcher, then perform
                // SQLite paging and DTO projection on a worker thread.
                var query = _libraryQueries.PrepareTrackPage(payload);
                return await Task.Run(() => _libraryQueries.TrackPage(query));
            }
            case "getGroups": return await Task.Run(() => _libraryQueries.GroupPage(payload));
            case "getFolders":
            {
                var scan = ScanDto();
                return await Task.Run(() => _libraryQueries.FolderData(payload, scan));
            }
            case "getArtistAlbums": return await Task.Run(() => _libraryQueries.ArtistAlbums(String(payload, "artist"), Int(payload, "offset"), Int(payload, "pageSize", 30)));
            case "getPlaylists": return await Task.Run(() => _libraryQueries.Playlists());
            case "getPlaylistTracks": return await Task.Run(() => PlaylistTrackPage(payload));
            case "getQueue": return QueuePage(payload);
            case "getCurrentTrack": return new
            {
                track = _playback.CurrentTrack is { } current ? _libraryQueries.TrackDto(current, true) : null,
                playing = _playback.IsPlaying, positionSeconds = Math.Max(0, _playback.Position) / 1000d,
                durationSeconds = Math.Max(0, _playback.Duration) / 1000d, volume = _playback.Volume, isVideo = _playback.IsVideoMode,
                shuffle = _shuffle, repeat = _repeatMode, repeatA = _repeatA?.TotalSeconds, repeatB = _repeatB?.TotalSeconds,
                queueIndex = _queueIndex
            };
            case "getLyrics":
            {
                var track = ResolveRequestedLyricsTrack(payload);
                return await Task.Run(() => LyricsData(track));
            }
            case "getAudioSettings": return await AudioSettingsAsync();
            case "getSettings": return WebSettings();
            case "getDuplicates": return await DuplicatePageAsync(payload);
            case "getDuplicateFiles": return await Task.Run(() => DuplicateFiles(payload));
            case "getTrackDetails": return await TrackDetailsDataAsync(TrackFrom(payload, "id"));
            case "getTags":
            {
                var track = TrackFrom(payload, "id");
                return await Task.Run(() => TagsData(track));
            }
            case "getExclusions": return new
            {
                paths = _libraryLocations.GetScanExclusions(),
                extensions = _libraryLocations.GetExtensionOptions().Select(option => new
                {
                    extension = option.Extension,
                    kind = option.Kind,
                    enabled = option.Enabled
                }).ToArray()
            };
            case "setView": _libraryQueries.SetContext(payload); return null;
            default: throw new InvalidOperationException("This Music Player command is not available in the data handler.");
        }
    }

    private bool _startupBackgroundWorkStarted;

    private static long Long(JsonElement payload, string key) =>
        payload.ValueKind == JsonValueKind.Object && payload.TryGetProperty(key, out var value) && value.TryGetInt64(out var result)
            ? result : 0;

    private static object AboutInfo()
    {
        var assembly = typeof(MainWindow).Assembly;
        var version = ApplicationVersion.Resolve(assembly);
        var noticesPath = Path.Combine(AppContext.BaseDirectory, "ThirdPartyNotices.md");
        var components = new List<object>();
        try
        {
            if (File.Exists(noticesPath))
            {
                var tableRows = File.ReadLines(noticesPath).Where(line => line.TrimStart().StartsWith('|'))
                    .Where(line => !line.Contains("---", StringComparison.Ordinal));
                foreach (var row in tableRows.Skip(1))
                {
                    var cells = row.Trim().Trim('|').Split('|').Select(cell => cell.Trim()).ToArray();
                    if (cells.Length < 5) continue;
                    var sources = Regex.Matches(cells[4], @"\[([^\]]+)\]\(https://[^)]+\)")
                        .Select(match => match.Groups[1].Value).ToArray();
                    components.Add(new { name = cells[0], use = cells[1], version = cells[2], license = cells[3], source = string.Join(", ", sources) });
                }
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            LocalAppLog.Shared.Warning("about", "Could not read the bundled third-party notices.", ex);
        }
        return new
        {
            version,
            repositoryUrl = GitHubUpdates.RepositoryUrl,
            releasesUrl = GitHubUpdates.ReleasesUrl,
            noticesAvailable = File.Exists(noticesPath),
            components
        };
    }
}
