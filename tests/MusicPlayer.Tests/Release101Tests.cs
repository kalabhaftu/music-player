using System.Reflection;
using System.Text.Json;
using Microsoft.Data.Sqlite;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class Release101Tests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-101-" + Guid.NewGuid().ToString("N"));
    private LibraryStore Store() => new(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
    private Track Track(string name, string? cover = null)
    {
        Directory.CreateDirectory(_root);
        var path = Path.Combine(_root, name + ".wav");
        File.WriteAllBytes(path, [1, 2, 3, 4]);
        return new(path, name, "Artist", "Album", "Artist", "Genre", 0, 0, TimeSpan.FromSeconds(3), 4,
            File.GetLastWriteTimeUtc(path), DateTime.UtcNow, ArtworkPath: cover);
    }

    [Theory]
    [InlineData("AddToQueue", "--add-to-queue", FileActivationAction.AddToQueue)]
    [InlineData("CreatePlaylist", "--create-playlist", FileActivationAction.CreatePlaylist)]
    public void File_verbs_and_forwarded_arguments_keep_the_action_and_duplicate_selection(string verb, string flag, FileActivationAction action)
    {
        var first = Track("first song"); var second = Track("second song");
        var request = FileActivation.Parse(["MusicPlayer.exe", flag, first.Path, second.Path, first.Path]);
        Assert.Equal(action, request.Action);
        Assert.Equal(new[] { first.Path, second.Path, first.Path }, request.Paths);
        var redirected = FileActivation.Parse([], verb, request.Paths);
        Assert.Equal(request.Action, redirected.Action);
        Assert.Equal(request.Paths, redirected.Paths);
    }

    [Fact]
    public void Deleted_paths_purge_references_compact_duplicates_and_cannot_return_in_checkpoints()
    {
        var store = Store(); var first = Track("first"); var gone = Track("gone"); var last = Track("last");
        store.UpsertTracks([first, gone, last]);
        var playlist = store.CreatePlaylist("Saved");
        store.AddToPlaylist(playlist.Id, [first.Path, gone.Path, last.Path, first.Path]);
        var session = new PlaybackSession(first.Path, 1234, [first.Path, gone.Path, last.Path, first.Path], false, "Off", QueueIndex: 3);
        store.SaveSession(session);
        store.RememberTrackPath(gone.Path);
        store.PurgeDeletedTracks([gone.Path]);
        Assert.Null(store.GetTrack(gone.Path));
        Assert.Null(store.ResolveTrackHandle(LibraryStore.TrackHandleId(gone.Path)));
        Assert.Equal(new[] { first.Path, last.Path, first.Path }, store.GetPlaylistPaths(playlist.Id));
        Assert.Equal(2, store.LoadSession()!.QueueIndex);
        store.SaveSession(session);
        Assert.Equal(new[] { first.Path, last.Path, first.Path }, store.LoadSession()!.Queue);
        store.PurgeDeletedTracks([first.Path]);
        store.SaveSessionState(session with { Queue = [], TrackPath = first.Path });
        Assert.Null(store.LoadSession()!.TrackPath);
        Assert.Equal(new[] { last.Path }, store.GetPlaylistPaths(playlist.Id));
        // Removing an index entry for an unselected root still preserves saved references.
        store.RemoveTracks([last.Path]);
        Assert.Equal(new[] { last.Path }, store.GetPlaylistPaths(playlist.Id));
    }

    [Fact]
    public void Shared_artwork_survives_until_the_last_reference_is_deleted()
    {
        var store = Store(); var art = Path.Combine(_root, "Artwork"); Directory.CreateDirectory(art);
        var cover = Path.Combine(art, "shared.jpg"); File.WriteAllBytes(cover, [1]);
        var a = Track("a", cover); var b = Track("b", cover); store.UpsertTracks([a, b]);
        store.PurgeDeletedTracks([a.Path]); store.PruneUnreferencedArtwork(art); Assert.True(File.Exists(cover));
        store.PurgeDeletedTracks([b.Path]); store.PruneUnreferencedArtwork(art); Assert.False(File.Exists(cover));
    }

    [Fact]
    public void Offline_visibility_agrees_for_rows_search_groups_counts_and_view_queues_and_reconnects()
    {
        var store = Store(); var a = Track("online"); var b = Track("offline"); store.UpsertTracks([a, b]);
        var queries = new LibraryQueryService(store, _root);
        queries.SetTrackUnavailable(b.Path, true);
        using var songs = JsonDocument.Parse("{\"view\":\"Songs\"}");
        var page = JsonSerializer.SerializeToElement(queries.TrackPage(songs.RootElement));
        Assert.Equal(1, page.GetProperty("totalCount").GetInt32());
        Assert.Equal(new[] { a.Path }, queries.CurrentTrackPaths());
        Assert.Equal(1, store.GetLibraryStats().TotalTracks);
        Assert.Equal(1, Assert.Single(store.GetGroupsPage("album")).TrackCount);
        Assert.Empty(store.GetTrackPaths(search: "offline"));
        Assert.Equal(2, store.GetTrackPaths(includeUnavailable: true).Count);
        queries.SetUnavailableRoots([_root]); Assert.Equal(0, store.GetLibraryStats().TotalTracks);
        queries.SetUnavailableRoots([]); queries.SetTrackUnavailable(b.Path, false); Assert.Equal(2, store.GetLibraryStats().TotalTracks);
    }

    [Fact]
    public void Browsing_shows_unresolved_duplicates_without_file_hashing_then_uses_verified_background_results()
    {
        var store = Store(); var a = Track("a"); var b = Track("b"); store.UpsertTracks([a, b]);
        var queries = new LibraryQueryService(store, _root);
        Assert.Equal(2, store.CountTracks(hideExactDuplicates: true));
        store.PrepareExactFingerprints();
        Assert.Equal(1, store.CountTracks(hideExactDuplicates: true));
        File.WriteAllBytes(b.Path, [5, 6, 7, 8]); store.UpsertTrack(b with { ModifiedUtc = File.GetLastWriteTimeUtc(b.Path) });
        Assert.Equal(2, store.CountTracks(hideExactDuplicates: true));
    }

    [Fact]
    public void Reconciliation_purges_deleted_folders_but_keeps_broken_link_boundaries()
    {
        var store = Store(); var track = Track("deleted"); store.UpsertTrack(track);
        var queries = new LibraryQueryService(store, _root);
        var locations = new LibraryLocationService(store);
        store.SetSetting("library-roots", JsonSerializer.Serialize(new[] { _root }));
        var availability = new TrackAvailabilityService(store, queries, locations, Path.Combine(_root, "Artwork"));
        var folder = Path.Combine(_root, "removed-folder"); Directory.CreateDirectory(folder);
        var child = track with { Path = Path.Combine(folder, "child.wav") }; File.WriteAllBytes(child.Path, [1]); store.UpsertTrack(child);
        var list = store.CreatePlaylist("Deleted folder", [child.Path, child.Path]);
        Directory.Delete(folder, true);
        availability.ReconcileRemovedPaths([folder]);
        Assert.Null(store.GetTrack(child.Path)); Assert.Empty(store.GetPlaylistPaths(list.Id));
        File.Delete(track.Path); availability.ReconcileIndexedTracks(); Assert.Null(store.GetTrack(track.Path));
        // Linux symlinks and Windows junctions both represent a mounted boundary.
        var link = Path.Combine(_root, "mount"); var target = Path.Combine(_root, "unmounted-target");
        if (OperatingSystem.IsWindows())
        {
            Directory.CreateDirectory(target);
            var start = new System.Diagnostics.ProcessStartInfo("cmd.exe") { UseShellExecute = false, CreateNoWindow = true };
            start.Arguments = "/c mklink /J \"" + link + "\" \"" + target + "\"";
            using var process = System.Diagnostics.Process.Start(start)!; process.WaitForExit(); Assert.Equal(0, process.ExitCode);
            Directory.Delete(target);
        }
        else Directory.CreateSymbolicLink(link, target);
        var offline = track with { Path = Path.Combine(link, "offline.wav") }; store.UpsertTrack(offline);
        Assert.False(availability.MarkUnavailable(offline.Path)); Assert.NotNull(store.GetTrack(offline.Path));
        // On Unix, Directory.Delete follows a dangling directory symlink and
        // reports that its target is missing; unlink the symlink itself.
        if (OperatingSystem.IsWindows()) Directory.Delete(link);
        else File.Delete(link);
    }

    [Theory]
    [InlineData("1.0.1", "v1.0.2", UpdateComparison.NewerRelease)]
    [InlineData("1.0.1", "v1.0.1", UpdateComparison.CurrentRelease)]
    [InlineData("1.0.1", null, UpdateComparison.NoEligibleRelease)]
    [InlineData("Unknown", "v1.0.1", UpdateComparison.UnknownVersion)]
    public void Update_comparison_only_reports_latest_after_a_valid_comparison(string current, string? latest, UpdateComparison expected)
        => Assert.Equal(expected, ApplicationVersion.Compare(current, latest));

    [Fact]
    public void Version_and_window_title_are_local_and_meaningful()
    {
        Assert.Equal("1.0.2", ApplicationVersion.Resolve(Assembly.GetExecutingAssembly()));
        Assert.Equal("Music Player", NativeWindowTitle.ForTrack(null));
        Assert.Equal("Song — Artist | Music Player", NativeWindowTitle.ForTrack(Track("Song")));
    }

    public void Dispose() { SqliteConnection.ClearAllPools(); if (Directory.Exists(_root)) Directory.Delete(_root, true); }
}
