using MusicPlayer.Core;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Windows.Media;

namespace MusicPlayer.App;

public sealed partial class MainWindow : Window
{
    private readonly LibraryStore _store;
    private readonly string _appData = AppDataPaths.Root;
    private readonly LibraryQueryService _libraryQueries;
    private readonly PlaylistLibraryService _playlistLibrary;
    private readonly PlayerSettingsService _playerSettings;
    private readonly LibraryLocationService _libraryLocations;
    private readonly TrackAvailabilityService _trackAvailability;
    private readonly LibraryFileWatcher _libraryFileWatcher;
    private readonly TrackMetadataService _trackMetadata;
    private readonly LibraryScanCoordinator _libraryScan;
    private readonly PlaybackQueueCoordinator _playbackQueue = new();
    private IReadOnlyList<string> _queue => _playbackQueue.Entries;
    private readonly SemaphoreSlim _externalFileActionGate = new(1, 1);
    private readonly TaskCompletionSource _shellReady = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly PlaybackService _playback;
    private readonly PlaybackCommandCoordinator _playbackCommands;
    private SystemMediaTransportControls? _systemControls;
    private TaskbarPeekControls? _taskbarPeekControls;
    private TrayIconService? _tray;
    private readonly DispatcherTimer _clock = new() { Interval = TimeSpan.FromMilliseconds(250) };
    private bool _shuffle { get => _playbackQueue.Shuffle; set => _playbackQueue.SetShuffle(value); }
    private bool _windowClosed;
    private readonly PlaybackListeningTracker _playbackListening = new();
    private DateTimeOffset? _scanStartedUtc;
    private string _scanCurrentPath = "";
    private string? _scanOutcomeKind;
    private string? _scanOutcomeMessage;
    private DateTimeOffset? _scanOutcomeUtc;
    private int _scanFilesFound;
    private int _scanDirectoriesVisited;
    private bool _crossfadeInProgress;
    private bool _libraryWatcherRescanPending;
    private string? _crossfadeFailureSource;
    private int? _crossfadeSourceQueueIndex;
    private int _queueIndex { get => _playbackQueue.CurrentIndex; set => _playbackQueue.SetCurrentIndex(value); }
    private DispatcherQueueTimer? _volumeSaveDebounce;
    private DispatcherQueueTimer? _noticeDismissTimer;
    private int? _pendingVolumeSetting;
    private int _persistedVolume;
    private int _crossfadeSeconds;
    private string _repeatMode { get => _playbackQueue.RepeatMode; set => _playbackQueue.SetRepeatMode(value); }
    private TimeSpan? _repeatA { get => _playbackQueue.RepeatA; set => _playbackQueue.SetAbRepeatPoints(value, _repeatB); }
    private TimeSpan? _repeatB { get => _playbackQueue.RepeatB; set => _playbackQueue.SetAbRepeatPoints(_repeatA, value); }
    private DateTime _lastSessionSave = DateTime.UtcNow;
    private WebViewBridge? _webBridge;
    private readonly List<nint> _windowIconHandles = [];
    private string? _nativeWindowIconPath;

    private void OnTracksPurged(IReadOnlyList<string> paths) => DispatcherQueue.TryEnqueue(() => ApplyPurgedTracks(paths));

    public MainWindow(LibraryStore store, PlaybackService? playback = null, bool openingExternalTrack = false)
    {
        _playback = playback ?? new PlaybackService();
        _store = store;
        _store.TracksPurged += OnTracksPurged;
        if (_store.GetSetting("accent-mode") is null) _store.SetSetting("accent-mode", "Artwork");
        InitializeComponent();
        ShellRoot.Loaded += (_, _) => _shellReady.TrySetResult();
        ShellRoot.ActualThemeChanged += (_, _) => ApplyNativeWindowChrome();
        _libraryQueries = new(_store, _appData);
        _libraryQueries.RestoreContext(_store.GetSetting("last-view-context"));
        _playbackCommands = new(_playbackQueue, _playback, _store.GetTrack, GetCurrentQueuePaths);
        _playlistLibrary = new(_store);
        _playerSettings = new(_store);
        if (_store.GetSetting("minimize-to-tray") is null) _store.SetSetting("minimize-to-tray", "true");
        _libraryLocations = new(_store);
        var videoExtensions = _libraryLocations.GetEnabledVideoExtensions();
        _playback.SetVideoExtensions(videoExtensions);
        _libraryQueries.SetVideoExtensions(videoExtensions);
        _playback.VideoSurfaceRequested = RequestVideoPlaybackSurface;
        _trackAvailability = new(_store, _libraryQueries, _libraryLocations, Path.Combine(_appData, "Artwork"));
        _libraryFileWatcher = new();
        _libraryFileWatcher.PathsRemoved += paths => _ = HandleLibraryPathsRemovedAsync(paths);
        _libraryFileWatcher.RescanRequested += RequestLibraryWatcherRescan;
        _trackMetadata = new(_appData);
        _libraryScan = new(new LibraryIndexer(_store, Path.Combine(_appData, "Artwork")));
        _volumeSaveDebounce = DispatcherQueue.CreateTimer();
        _volumeSaveDebounce.Interval = TimeSpan.FromMilliseconds(250);
        _volumeSaveDebounce.IsRepeating = false;
        _volumeSaveDebounce.Tick += (_, _) => PersistPendingVolumeSetting();
        var volume = int.TryParse(_store.GetSetting("volume"), out var savedVolume) ? Math.Clamp(savedVolume, 0, 100) : 75;
        _persistedVolume = volume;
        _crossfadeSeconds = ReadCrossfadeSeconds();
        _playback.Volume = volume;
        _playback.SelectAudioOutputDevice(_store.GetSetting("audio-output-device"));
        if (_playback.AudioOutputDeviceId.Length == 0)
        {
            _store.SetSetting("audio-output-device", "");
            _store.SetSetting("audio-output-name", "System default");
        }
        _playback.AudioOutputFallback += (_, _) => DispatcherQueue.TryEnqueue(() =>
        {
            if (_windowClosed) return;
            if (!_playback.ApplyPendingAudioOutputFallback(CancelCrossfadeAndRestoreQueue)) return;
            _store.SetSetting("audio-output-device", "");
            _store.SetSetting("audio-output-name", "System default");
            _webBridge?.SendEvent("audioOutputChanged", new { selectedId = "", selectedName = "System default" });
        });
        ApplyStoredEqualizer();
        _playback.TrackEnded += Playback_TrackEnded;
        _playback.CrossfadeCompleted += Playback_CrossfadeCompleted;
        _playback.CrossfadeFailed += Playback_CrossfadeFailed;
        _playback.PlaybackFailed += Playback_Failed;
        InitializeSystemMediaControls();
        InitializeTaskbarPeekControls();
        ApplyTraySetting();
        _clock.Tick += Clock_Tick;
        _clock.Start();
        _noticeDismissTimer = DispatcherQueue.CreateTimer();
        _noticeDismissTimer.Interval = TimeSpan.FromSeconds(4);
        _noticeDismissTimer.IsRepeating = false;
        _noticeDismissTimer.Tick += (_, _) => AppNotice.IsOpen = false;
        Closed += MainWindow_Closed;
        ApplyStoredAppearance();
        Activated += (_, args) =>
        {
            if (args.WindowActivationState != WindowActivationState.Deactivated) _ = RefreshDefaultAppsAsync();
            ApplyNativeWindowIcon();
            ApplyNativeWindowChrome();
        };
        RestoreSession(loadMedia: !openingExternalTrack);
        WebUi.Loaded += WebUi_Loaded;
    }

}
