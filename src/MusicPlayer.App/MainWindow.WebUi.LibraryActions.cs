using System.Diagnostics;
using MusicPlayer.Core;
using Microsoft.UI.Xaml;
using Windows.Devices.Enumeration;
using Windows.Media.Devices;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task AddFolderFromWebAsync()
    {
        var picker = new FolderPicker(); picker.FileTypeFilter.Add("*");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var folder = await picker.PickSingleFolderAsync(); if (folder is null) return;
        var path = _libraryLocations.AddLibraryRoot(folder.Path);
        _libraryFileWatcher.SetRoots(_libraryLocations.GetLibraryRoots(), _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        StartScan([path]); PublishLibraryChanged();
    }

    private async Task RemoveRootFromWebAsync(string rawPath)
    {
        var removal = _libraryLocations.PrepareLibraryRootRemoval(rawPath);
        if (removal is null) return;
        await Task.Run(() => _store.RemoveTracksUnderUnselectedRoots(removal.RemovedRoots, removal.RemainingRoots));
        await Task.Run(() => _store.PruneUnreferencedArtwork(Path.Combine(_appData, "Artwork")));
        _libraryLocations.PersistLibraryRootRemoval(removal);
        _libraryFileWatcher.SetRoots(removal.RemainingRoots, _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        PublishLibraryChanged();
    }

    private async Task ImportPlaylistAsync()
    {
        var picker = new FileOpenPicker(); picker.FileTypeFilter.Add(".m3u"); picker.FileTypeFilter.Add(".m3u8");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var file = await picker.PickSingleFileAsync(); if (file is null) return;
        _store.ImportM3u8(file.Path); PublishLibraryChanged();
    }

    private async Task ExportPlaylistAsync(string id)
    {
        var playlist = _store.GetPlaylistSummaries().FirstOrDefault(item => item.Id == id) ?? throw new KeyNotFoundException("Playlist not found.");
        var picker = new FileSavePicker(); picker.FileTypeChoices.Add("M3U8 playlist", [".m3u8"]); picker.SuggestedFileName = playlist.Name;
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var file = await picker.PickSaveFileAsync(); if (file is not null) _store.ExportM3u8(id, file.Path);
    }

    private async Task AddExclusionAsync()
    {
        var picker = new FolderPicker(); picker.FileTypeFilter.Add("*"); InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var folder = await picker.PickSingleFolderAsync(); if (folder is null) return;
        _libraryLocations.AddScanExclusion(folder.Path);
        _libraryQueries.SetExcludedDirectories(_libraryLocations.GetScanExclusions());
        await Task.Run(() => _store.PurgeTracksUnderExcludedDirectories([folder.Path]));
        _libraryFileWatcher.SetRoots(_libraryLocations.GetLibraryRoots(), _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        PublishLibraryChanged("exclusionAdded");

    }

    private void RemoveExclusion(string path)
    {
        _libraryLocations.RemoveScanExclusion(path);
        _libraryQueries.SetExcludedDirectories(_libraryLocations.GetScanExclusions());
        _libraryFileWatcher.SetRoots(_libraryLocations.GetLibraryRoots(), _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        var roots = _libraryLocations.GetLibraryRoots();
        if (roots.Length > 0) StartScan(roots);
        else PublishLibraryChanged("library");
    }
}
