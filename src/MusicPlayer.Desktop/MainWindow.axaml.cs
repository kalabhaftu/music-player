using System;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using MusicPlayer.Desktop.Models;
using MusicPlayer.Desktop.ViewModels;

namespace MusicPlayer.Desktop;

public partial class MainWindow : Window
{
    private MainWindowViewModel? ViewModel => DataContext as MainWindowViewModel;

    public MainWindow()
    {
        InitializeComponent();
        DataContext = new MainWindowViewModel();
    }

    private void OnHomeClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Home);
    private void OnSearchNavClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Search);
    private void OnFavoritesClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Favorites);
    private void OnSongsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Songs);
    private void OnAlbumsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Albums);
    private void OnArtistsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Artists);
    private void OnRecentlyAddedClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.RecentlyAdded);
    private void OnPlaylistsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Playlists);
    private void OnFoldersClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Folders);
    private void OnSettingsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Settings);
    private void OnLyricsClicked(object? sender, RoutedEventArgs e) => ViewModel?.Navigate(NavViewKind.Lyrics);

    private void OnToggleSidebarClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleSidebar();
    private void OnToggleRightPanelClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleRightPanel();
    private void OnToggleImmersiveClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleImmersive();
    private void OnToggleImmersiveLyricsClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleImmersiveLyrics();

    private void OnRightPanelQueueClicked(object? sender, RoutedEventArgs e) => ViewModel?.SetRightPanelTab("queue");
    private void OnRightPanelInfoClicked(object? sender, RoutedEventArgs e) => ViewModel?.SetRightPanelTab("info");

    private void OnTrackDoubleTapped(object? sender, TappedEventArgs e)
    {
        if (sender is ListBox listBox && listBox.SelectedItem is UiTrack track)
        {
            ViewModel?.PlayTrack(track);
        }
    }

    private void OnTrackHeartClicked(object? sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.DataContext is UiTrack track)
        {
            ViewModel?.ToggleFavorite(track);
        }
    }

    private void OnPlayAlbumClicked(object? sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.DataContext is UiAlbum album)
        {
            ViewModel?.PlayAlbum(album);
        }
    }

    private void OnPlayArtistClicked(object? sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.DataContext is UiArtist artist)
        {
            ViewModel?.PlayArtist(artist);
        }
    }

    private void OnPlayPlaylistClicked(object? sender, RoutedEventArgs e)
    {
        if (sender is Control ctrl && ctrl.DataContext is UiPlaylist playlist)
        {
            ViewModel?.PlayPlaylist(playlist);
        }
    }

    private void OnPlayAllClicked(object? sender, RoutedEventArgs e) => ViewModel?.PlayAll();
    private void OnShuffleAllClicked(object? sender, RoutedEventArgs e) => ViewModel?.ShuffleAll();

    private void OnPlayPauseClicked(object? sender, RoutedEventArgs e) => ViewModel?.TogglePlayPause();
    private void OnNextClicked(object? sender, RoutedEventArgs e) => ViewModel?.Next();
    private void OnPreviousClicked(object? sender, RoutedEventArgs e) => ViewModel?.Previous();
    private void OnShuffleClicked(object? sender, RoutedEventArgs e)
    {
        if (ViewModel != null) ViewModel.Shuffle = !ViewModel.Shuffle;
    }
    private void OnRepeatClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleRepeat();
    private void OnFavoriteClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleFavorite(null);
    private void OnMuteClicked(object? sender, RoutedEventArgs e) => ViewModel?.ToggleMute();

    private void OnClearQueueClicked(object? sender, RoutedEventArgs e) => ViewModel?.ClearQueue();

    private void OnLyricLineTapped(object? sender, TappedEventArgs e)
    {
        if (sender is Control ctrl && ctrl.DataContext is LyricLine line)
        {
            ViewModel?.Seek(line.TimeSeconds);
        }
    }

    private void OnJumpToCurrentLyricClicked(object? sender, RoutedEventArgs e)
    {
        ViewModel?.JumpToCurrentLyric();
    }

    private void OnSeekPointerReleased(object? sender, PointerCaptureLostEventArgs e)
    {
        if (sender is Slider slider)
        {
            ViewModel?.Seek(slider.Value);
        }
    }
}
