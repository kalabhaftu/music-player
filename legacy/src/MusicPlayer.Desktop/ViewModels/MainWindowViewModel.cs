using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using MusicPlayer.Core;
using MusicPlayer.Desktop.Models;
using MusicPlayer.Desktop.Services;

namespace MusicPlayer.Desktop.ViewModels;

public sealed class MainWindowViewModel : ViewModelBase
{
    private readonly DesktopLibraryService _libraryService;
    private readonly DesktopPlaybackService _playbackService;

    private NavViewKind _activeView = NavViewKind.Playlists;
    private bool _isSidebarExpanded = true;
    private bool _isRightPanelExpanded = true;
    private bool _isImmersivePlayerOpen = false;
    private bool _isImmersiveLyricsOpen = false;
    private string _rightPanelTab = "queue";
    private string _searchQuery = string.Empty;
    private UiTrack? _currentTrack;
    private bool _isPlaying;
    private double _positionSeconds;
    private double _durationSeconds;
    private string _positionFormatted = "0:00";
    private string _durationFormatted = "0:00";
    private int _volume = 75;
    private bool _isMuted;
    private bool _shuffle;
    private RepeatMode _repeat = RepeatMode.Off;
    private int _activeLyricIndex = -1;

    public MainWindowViewModel()
    {
        _libraryService = new DesktopLibraryService();
        _playbackService = new DesktopPlaybackService();

        Tracks = new ObservableCollection<UiTrack>();
        Albums = new ObservableCollection<UiAlbum>();
        Artists = new ObservableCollection<UiArtist>();
        Playlists = new ObservableCollection<UiPlaylist>();
        SmartPlaylists = new ObservableCollection<UiPlaylist>();
        Folders = new ObservableCollection<string>();
        Queue = new ObservableCollection<UiTrack>();
        LyricsLines = new ObservableCollection<LyricLine>();

        _playbackService.TrackChanged += OnTrackChanged;
        _playbackService.PlaybackStateChanged += state => IsPlaying = state;
        _playbackService.PositionChanged += OnPositionChanged;
        _playbackService.QueueChanged += OnQueueChanged;
        _playbackService.VolumeChanged += vol =>
        {
            Volume = vol;
            IsMuted = _playbackService.IsMuted;
        };

        LoadLibraryData();
    }

    public ObservableCollection<UiTrack> Tracks { get; }
    public ObservableCollection<UiAlbum> Albums { get; }
    public ObservableCollection<UiArtist> Artists { get; }
    public ObservableCollection<UiPlaylist> Playlists { get; }
    public ObservableCollection<UiPlaylist> SmartPlaylists { get; }
    public ObservableCollection<string> Folders { get; }
    public ObservableCollection<UiTrack> Queue { get; }
    public ObservableCollection<LyricLine> LyricsLines { get; }

    public NavViewKind ActiveView
    {
        get => _activeView;
        set
        {
            if (SetProperty(ref _activeView, value))
            {
                OnPropertyChanged(nameof(IsHomeActive));
                OnPropertyChanged(nameof(IsSearchActive));
                OnPropertyChanged(nameof(IsFavoritesActive));
                OnPropertyChanged(nameof(IsSongsActive));
                OnPropertyChanged(nameof(IsAlbumsActive));
                OnPropertyChanged(nameof(IsArtistsActive));
                OnPropertyChanged(nameof(IsRecentlyAddedActive));
                OnPropertyChanged(nameof(IsPlaylistsActive));
                OnPropertyChanged(nameof(IsFoldersActive));
                OnPropertyChanged(nameof(IsSettingsActive));
                OnPropertyChanged(nameof(IsLyricsActive));
                OnPropertyChanged(nameof(ViewTitle));
                OnPropertyChanged(nameof(ViewSubtitle));
                RefreshCurrentViewData();
            }
        }
    }

    public bool IsHomeActive => ActiveView == NavViewKind.Home;
    public bool IsSearchActive => ActiveView == NavViewKind.Search;
    public bool IsFavoritesActive => ActiveView == NavViewKind.Favorites;
    public bool IsSongsActive => ActiveView == NavViewKind.Songs;
    public bool IsAlbumsActive => ActiveView == NavViewKind.Albums;
    public bool IsArtistsActive => ActiveView == NavViewKind.Artists;
    public bool IsRecentlyAddedActive => ActiveView == NavViewKind.RecentlyAdded;
    public bool IsPlaylistsActive => ActiveView == NavViewKind.Playlists;
    public bool IsFoldersActive => ActiveView == NavViewKind.Folders;
    public bool IsSettingsActive => ActiveView == NavViewKind.Settings;
    public bool IsLyricsActive => ActiveView == NavViewKind.Lyrics;

    public string ViewTitle => ActiveView switch
    {
        NavViewKind.Home => "Home",
        NavViewKind.Search => "Search Results",
        NavViewKind.Favorites => "Favorites",
        NavViewKind.Songs => $"All Songs ({Tracks.Count})",
        NavViewKind.Albums => $"Albums ({Albums.Count})",
        NavViewKind.Artists => $"Artists ({Artists.Count})",
        NavViewKind.RecentlyAdded => "Recently Added",
        NavViewKind.Playlists => "Playlists",
        NavViewKind.Folders => "Music Folders",
        NavViewKind.Settings => "Settings",
        NavViewKind.Lyrics => "Lyrics",
        _ => "Library"
    };

    public string ViewSubtitle => ActiveView switch
    {
        NavViewKind.Playlists => "Your playlists are stored on this PC.",
        NavViewKind.Songs => $"{Tracks.Count} {(Tracks.Count == 1 ? "song" : "songs")}",
        NavViewKind.Favorites => $"{Tracks.Count} {(Tracks.Count == 1 ? "favorite song" : "favorite songs")}",
        NavViewKind.RecentlyAdded => "Recently added to your local library",
        NavViewKind.Albums => $"{Albums.Count} {(Albums.Count == 1 ? "album" : "albums")}",
        NavViewKind.Artists => $"{Artists.Count} {(Artists.Count == 1 ? "artist" : "artists")}",
        _ => ""
    };

    public bool IsSidebarExpanded
    {
        get => _isSidebarExpanded;
        set => SetProperty(ref _isSidebarExpanded, value);
    }

    public bool IsRightPanelExpanded
    {
        get => _isRightPanelExpanded;
        set => SetProperty(ref _isRightPanelExpanded, value);
    }

    public bool IsImmersivePlayerOpen
    {
        get => _isImmersivePlayerOpen;
        set => SetProperty(ref _isImmersivePlayerOpen, value);
    }

    public bool IsImmersiveLyricsOpen
    {
        get => _isImmersiveLyricsOpen;
        set => SetProperty(ref _isImmersiveLyricsOpen, value);
    }

    public string RightPanelTab
    {
        get => _rightPanelTab;
        set
        {
            if (SetProperty(ref _rightPanelTab, value))
            {
                OnPropertyChanged(nameof(IsQueueTabActive));
                OnPropertyChanged(nameof(IsTrackInfoTabActive));
            }
        }
    }

    public bool IsQueueTabActive => string.Equals(RightPanelTab, "queue", StringComparison.OrdinalIgnoreCase);
    public bool IsTrackInfoTabActive => string.Equals(RightPanelTab, "info", StringComparison.OrdinalIgnoreCase);

    public string SearchQuery
    {
        get => _searchQuery;
        set
        {
            if (SetProperty(ref _searchQuery, value))
            {
                PerformSearch(value);
            }
        }
    }

    public UiTrack? CurrentTrack
    {
        get => _currentTrack;
        private set
        {
            if (SetProperty(ref _currentTrack, value))
            {
                OnPropertyChanged(nameof(HasCurrentTrack));
                OnPropertyChanged(nameof(CurrentTitle));
                OnPropertyChanged(nameof(CurrentArtist));
                OnPropertyChanged(nameof(CurrentAlbum));
                OnPropertyChanged(nameof(CurrentTrackIsFavorite));
                OnPropertyChanged(nameof(CurrentArtworkBitmap));
                OnPropertyChanged(nameof(CurrentHasArtwork));
                OnPropertyChanged(nameof(CurrentInitials));
                OnPropertyChanged(nameof(CurrentCoverBrush));
                OnPropertyChanged(nameof(CurrentSubtitle));
            }
        }
    }

    public bool HasCurrentTrack => CurrentTrack != null;
    public string CurrentTitle => CurrentTrack?.Title ?? "Nothing playing";
    public string CurrentArtist => CurrentTrack?.Artist ?? "Choose a song from your library";
    public string CurrentAlbum => CurrentTrack?.Album ?? "";
    public string CurrentSubtitle => CurrentTrack?.Subtitle ?? "Choose a song from your library";
    public bool CurrentTrackIsFavorite => CurrentTrack?.IsFavorite ?? false;
    public Bitmap? CurrentArtworkBitmap => CurrentTrack?.ArtworkBitmap;
    public bool CurrentHasArtwork => CurrentTrack?.HasArtwork ?? false;
    public string CurrentInitials => CurrentTrack?.Initials ?? "MP";
    public IBrush CurrentCoverBrush => CurrentTrack?.CoverBrush ?? ArtworkHelper.CoverGradient1;

    public bool IsPlaying
    {
        get => _isPlaying;
        set => SetProperty(ref _isPlaying, value);
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        set
        {
            if (SetProperty(ref _positionSeconds, value))
            {
                PositionFormatted = FormatTime(value);
            }
        }
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        set
        {
            if (SetProperty(ref _durationSeconds, value))
            {
                DurationFormatted = FormatTime(value);
            }
        }
    }

    public string PositionFormatted
    {
        get => _positionFormatted;
        private set => SetProperty(ref _positionFormatted, value);
    }

    public string DurationFormatted
    {
        get => _durationFormatted;
        private set => SetProperty(ref _durationFormatted, value);
    }

    public int Volume
    {
        get => _volume;
        set
        {
            if (SetProperty(ref _volume, value))
            {
                _playbackService.SetVolume(value);
            }
        }
    }

    public bool IsMuted
    {
        get => _isMuted;
        set => SetProperty(ref _isMuted, value);
    }

    public bool Shuffle
    {
        get => _shuffle;
        set
        {
            if (SetProperty(ref _shuffle, value))
            {
                _playbackService.Shuffle = value;
            }
        }
    }

    public RepeatMode Repeat
    {
        get => _repeat;
        set
        {
            if (SetProperty(ref _repeat, value))
            {
                _playbackService.Repeat = value;
                OnPropertyChanged(nameof(RepeatText));
                OnPropertyChanged(nameof(IsRepeatActive));
            }
        }
    }

    public bool IsRepeatActive => Repeat != RepeatMode.Off;

    public string RepeatText => Repeat switch
    {
        RepeatMode.All => "Repeat: All",
        RepeatMode.One => "Repeat: One",
        _ => "Repeat: Off"
    };

    public void Navigate(NavViewKind kind)
    {
        ActiveView = kind;
    }

    public void ToggleSidebar() => IsSidebarExpanded = !IsSidebarExpanded;
    public void ToggleRightPanel() => IsRightPanelExpanded = !IsRightPanelExpanded;
    public void ToggleImmersive() => IsImmersivePlayerOpen = !IsImmersivePlayerOpen;
    public void ToggleImmersiveLyrics() => IsImmersiveLyricsOpen = !IsImmersiveLyricsOpen;
    public void SetRightPanelTab(string tab) => RightPanelTab = tab;

    public void PlayTrack(UiTrack track)
    {
        _playbackService.PlayTrack(track, Tracks);
    }

    public void PlayAlbum(UiAlbum album)
    {
        var albumTracks = _libraryService.GetTracksForAlbum(album.Title, album.Artist);
        if (albumTracks.Count > 0)
        {
            _playbackService.PlayTrack(albumTracks[0], albumTracks);
        }
    }

    public void PlayArtist(UiArtist artist)
    {
        var artistTracks = _libraryService.GetTracksForArtist(artist.Name);
        if (artistTracks.Count > 0)
        {
            _playbackService.PlayTrack(artistTracks[0], artistTracks);
        }
    }

    public void PlayPlaylist(UiPlaylist playlist)
    {
        if (playlist.IsSmart)
        {
            if (playlist.Id == "smart-mp")
            {
                RefreshTracks(_libraryService.GetAllTracks());
            }
            else if (playlist.Id == "smart-wl")
            {
                RefreshTracks(_libraryService.GetAllTracks().Where(t => !string.IsNullOrEmpty(t.Path) && (LyricsFiles.HasUsableSidecar(t.Path))).ToList());
            }
            else if (playlist.Id == "smart-rp")
            {
                RefreshTracks(_libraryService.GetRecentlyAddedTracks());
            }
            ActiveView = NavViewKind.Songs;
            if (Tracks.Count > 0) PlayTrack(Tracks[0]);
        }
    }

    public void PlayAll()
    {
        if (Tracks.Count > 0)
        {
            PlayTrack(Tracks[0]);
        }
    }

    public void ShuffleAll()
    {
        if (Tracks.Count > 0)
        {
            Shuffle = true;
            var rand = new Random().Next(Tracks.Count);
            PlayTrack(Tracks[rand]);
        }
    }

    public void TogglePlayPause() => _playbackService.TogglePlayPause();
    public void Next() => _playbackService.Next();
    public void Previous() => _playbackService.Previous();
    public void Seek(double seconds) => _playbackService.Seek(seconds);
    public void ToggleMute() => _playbackService.ToggleMute();

    public void ToggleRepeat()
    {
        Repeat = Repeat switch
        {
            RepeatMode.Off => RepeatMode.All,
            RepeatMode.All => RepeatMode.One,
            _ => RepeatMode.Off
        };
    }

    public void ToggleFavorite(UiTrack? track)
    {
        track ??= CurrentTrack;
        if (track == null) return;

        track.IsFavorite = !track.IsFavorite;
        _libraryService.SetFavorite(track.Path, track.IsFavorite);
        OnPropertyChanged(nameof(CurrentTrackIsFavorite));

        if (ActiveView == NavViewKind.Favorites)
        {
            RefreshCurrentViewData();
        }
    }

    public void Enqueue(UiTrack track) => _playbackService.Enqueue(track);
    public void EnqueueNext(UiTrack track) => _playbackService.EnqueueNext(track);
    public void RemoveFromQueue(int index) => _playbackService.RemoveFromQueue(index);
    public void ClearQueue() => _playbackService.ClearQueue();

    public void AddFolder(string path)
    {
        if (string.IsNullOrWhiteSpace(path) || !Directory.Exists(path)) return;
        _libraryService.AddLibraryFolder(path);
        LoadLibraryData();
    }

    public void RemoveFolder(string path)
    {
        _libraryService.RemoveLibraryFolder(path);
        LoadLibraryData();
    }

    public void JumpToCurrentLyric()
    {
        // Resumes sync focus
    }

    private void LoadLibraryData()
    {
        RefreshTracks(_libraryService.GetAllTracks());

        Albums.Clear();
        foreach (var a in _libraryService.GetAlbums()) Albums.Add(a);

        Artists.Clear();
        foreach (var a in _libraryService.GetArtists()) Artists.Add(a);

        Playlists.Clear();
        foreach (var p in _libraryService.GetPlaylists()) Playlists.Add(p);

        SmartPlaylists.Clear();
        foreach (var sp in _libraryService.GetSmartPlaylists()) SmartPlaylists.Add(sp);

        Folders.Clear();
        foreach (var f in _libraryService.GetLibraryFolders()) Folders.Add(f);

        OnPropertyChanged(nameof(ViewTitle));
        OnPropertyChanged(nameof(ViewSubtitle));
    }

    private void RefreshCurrentViewData()
    {
        switch (ActiveView)
        {
            case NavViewKind.Favorites:
                RefreshTracks(_libraryService.GetFavoriteTracks());
                break;
            case NavViewKind.RecentlyAdded:
                RefreshTracks(_libraryService.GetRecentlyAddedTracks());
                break;
            case NavViewKind.Songs:
            case NavViewKind.Home:
            default:
                RefreshTracks(_libraryService.GetAllTracks());
                break;
        }
    }

    private void RefreshTracks(IReadOnlyList<UiTrack> newTracks)
    {
        Tracks.Clear();
        foreach (var t in newTracks) Tracks.Add(t);
        OnPropertyChanged(nameof(ViewTitle));
        OnPropertyChanged(nameof(ViewSubtitle));
    }

    private void PerformSearch(string query)
    {
        if (string.IsNullOrWhiteSpace(query))
        {
            RefreshCurrentViewData();
            return;
        }

        ActiveView = NavViewKind.Search;
        var results = _libraryService.Search(query);
        RefreshTracks(results);
    }

    private void OnTrackChanged(UiTrack? track)
    {
        CurrentTrack = track;
        LoadLyrics(track);
    }

    private void OnPositionChanged(double position, double duration)
    {
        PositionSeconds = position;
        DurationSeconds = duration;
        UpdateActiveLyric(position);
    }

    private void OnQueueChanged()
    {
        Queue.Clear();
        foreach (var item in _playbackService.Queue)
        {
            Queue.Add(item);
        }
    }

    private void LoadLyrics(UiTrack? track)
    {
        LyricsLines.Clear();
        _activeLyricIndex = -1;
        if (track == null || string.IsNullOrEmpty(track.Path)) return;

        try
        {
            var result = LyricsFiles.Read(track.Path);
            if (!string.IsNullOrEmpty(result.Text))
            {
                var doc = Lyrics.Parse(result.Text);
                foreach (var line in doc.Lines)
                {
                    LyricsLines.Add(new LyricLine
                    {
                        TimeSeconds = line.Time.TotalSeconds,
                        Text = line.Text
                    });
                }
            }
        }
        catch
        {
            // Fallback: no lyrics
        }
    }

    private void UpdateActiveLyric(double positionSeconds)
    {
        if (LyricsLines.Count == 0) return;

        int newIdx = -1;
        for (int i = 0; i < LyricsLines.Count; i++)
        {
            if (LyricsLines[i].TimeSeconds <= positionSeconds)
            {
                newIdx = i;
            }
            else break;
        }

        if (newIdx != _activeLyricIndex)
        {
            _activeLyricIndex = newIdx;
            for (int i = 0; i < LyricsLines.Count; i++)
            {
                LyricsLines[i].IsActive = (i == _activeLyricIndex);
                LyricsLines[i].IsPast = (i < _activeLyricIndex);
            }
        }
    }

    private static string FormatTime(double seconds)
    {
        if (seconds < 0) seconds = 0;
        var ts = TimeSpan.FromSeconds(seconds);
        return ts.Hours > 0 
            ? ts.ToString(@"h\:mm\:ss") 
            : ts.ToString(@"m\:ss");
    }
}
