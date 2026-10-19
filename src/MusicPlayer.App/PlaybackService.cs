using MusicPlayer.Core;
using LibVLCSharp.Shared;
using LibVLCSharp.Shared.Structures;

namespace MusicPlayer.App;

public sealed record VideoSubtitleOption(int Id, string Name);

/// <summary>Serializes LibVLC state changes and creates the crossfade player only when it is first needed.</summary>
public sealed class PlaybackService : IDisposable
{
    private readonly object _gate = new();
    private readonly LibVLC _libVlc;
    private readonly bool _independentMusicOutput;
    private readonly Timer _outputMonitor;
    private HashSet<string> _videoExtensions = new(StringComparer.OrdinalIgnoreCase);
    private readonly MediaPlayer _first;
    private MediaPlayer _active;
    private MediaPlayer? _spare;
    private sealed class PlayerSubscription
    {
        public long Generation;
        public long VolumeWorkerGeneration = -1;
        public EventHandler<EventArgs> End = null!;
        public EventHandler<EventArgs> Error = null!;
        public EventHandler<EventArgs> Playing = null!;
    }

    private readonly Dictionary<MediaPlayer, PlayerSubscription> _subscriptions = [];
    private readonly Dictionary<MediaPlayer, (string Output, string Device)> _audioRoutes = [];
    private Media? _activeMedia;
    private Media? _spareMedia;
    private readonly Dictionary<MediaPlayer, DffStreamInput> _fileInputs = [];

    private sealed class DffStreamInput(FileStream stream, long duration) : StreamMediaInput(stream)
    {
        public long Duration { get; } = duration;
        public override bool Open(out ulong size)
        {
            var opened = base.Open(out size);
            // LibVLC's file reader rejects seeking exactly to EOF while FFmpeg
            // parses DFF. Its documented unknown-size stream mode accepts it.
            size = ulong.MaxValue;
            return opened;
        }
        protected override void Dispose(bool disposing)
        {
            base.Dispose(disposing);
            if (disposing) stream.Dispose();
        }
    }
    private Track? _currentTrack;
    private Track? _crossfadeTarget;
    private CancellationTokenSource? _crossfadeCancellation;
    private double _crossfadeProgress;
    private string? _reportedFailurePath;
    private string? _audioOutputDeviceId;
    private string _directSoundDeviceId = "";
    private string? _equalizerPreset;
    private IReadOnlyList<float>? _equalizerBands;
    public const float NeutralEqualizerPreamp = 12.0412f;
    private float _equalizerPreamp = NeutralEqualizerPreamp;
    public float EqualizerPreamp { get { lock (_gate) return _equalizerPreamp; } }
    private float _volume = 75;
    private float _videoPlaybackRate = 1f;
    private long _restorePosition;
    private long _mediaGeneration;
    private long _crossfadeCommittedGeneration = -1;
    private long _handledEndReachedGeneration = -1;
    private bool _trackEnded;
    private bool _videoTrack;
    private bool _playRequested;
    private bool _disposed;
    private int _checkingOutput;
    private string? _pendingFallbackDeviceId;

    // VLC 3's MMDevice volume belongs to the shared Windows session. DirectSound
    // controls each music player's buffer independently, which crossfade needs.
    public PlaybackService() : this("--aout=directsound", "--directx-volume=0", "--no-video-title-show", "--no-volume-save", "--no-equalizer-2pass", "--no-equalizer-vlcfreqs") { }

    internal PlaybackService(params string[] options)
    {
        LibVLCSharp.Shared.Core.Initialize();
        _independentMusicOutput = options.Contains("--aout=directsound", StringComparer.Ordinal);
        // Video is rendered into the app's own child HWND when an enabled video
        // extension is played. Never let LibVLC create its standalone video window.
        _libVlc = new LibVLC(options);
        _libVlc.Log += (_, args) =>
        {
            if (args.Level is LogLevel.Warning or LogLevel.Error)
                LocalAppLog.Shared.Warning("libvlc", args.FormattedLog);
        };
        _first = new(_libVlc);
        _active = _first;
        Subscribe(_first);
        _outputMonitor = new Timer(_ => CheckAudioOutput(), null, TimeSpan.FromSeconds(2), TimeSpan.FromSeconds(2));
    }

    public event EventHandler? TrackEnded;
    public event Action<Track>? CrossfadeCompleted;
    public event Action<Track>? CrossfadeFailed;
    public event Action<Track>? PlaybackFailed;
    public event EventHandler? AudioOutputFallback;
    public bool ApplyPendingAudioOutputFallback(Action beforeFallback)
    {
        lock (_gate)
        {
            if (_disposed || _pendingFallbackDeviceId is null || _audioOutputDeviceId != _pendingFallbackDeviceId) return false;
            _pendingFallbackDeviceId = null;
            beforeFallback();
            SelectAudioOutputDevice(null);
            return true;
        }
    }
    public string AudioOutputDeviceId { get { lock (_gate) return _audioOutputDeviceId ?? ""; } }
    public Func<Track, nint>? VideoSurfaceRequested { get; set; }

    public Track? CurrentTrack { get { lock (_gate) return _currentTrack; } }
    public bool IsPlaying { get { lock (_gate) return !_disposed && _active.IsPlaying; } }
    public bool IsCurrentPlaybackFailure(Track track)
    {
        lock (_gate) return !_disposed && !_active.IsPlaying &&
            string.Equals(_reportedFailurePath, track.Path, StringComparison.OrdinalIgnoreCase) &&
            string.Equals(_currentTrack?.Path, track.Path, StringComparison.OrdinalIgnoreCase);
    }
    public bool HasEnded { get { lock (_gate) return !_disposed && _trackEnded; } }
    public bool HasCommittedCrossfade { get { lock (_gate) return !_disposed && _crossfadeCommittedGeneration == _mediaGeneration; } }
    public bool IsVideoMode { get { lock (_gate) return !_disposed && _videoTrack; } }
    public long Position { get { lock (_gate) return _disposed ? 0 : _restorePosition > _active.Time ? _restorePosition : _active.Time; } }
    public long Duration
    {
        get
        {
            lock (_gate)
            {
                if (_disposed) return 0;
                var engineDuration = _active.Length;
                if (engineDuration > 0) return engineDuration;
                if (_fileInputs.TryGetValue(_active, out var input)) return input.Duration;
                return Math.Max(0, (long)(_currentTrack?.Duration.TotalMilliseconds ?? 0));
            }
        }
    }
    public int Volume
    {
        get { lock (_gate) return (int)_volume; }
        set
        {
            lock (_gate)
            {
                if (_disposed) return;
                _volume = Math.Clamp(value, 0, 100);
                ApplyCrossfadeVolumeCore();
            }
        }
    }

    public float VideoPlaybackRate { get { lock (_gate) return _videoPlaybackRate; } }
    public int VideoSubtitleId { get { lock (_gate) return _disposed || !_videoTrack ? -1 : _active.Spu; } }

    public bool SetVideoPlaybackRate(float rate)
    {
        rate = Math.Clamp(rate, .5f, 2f);
        lock (_gate)
        {
            if (_disposed || !_videoTrack) return false;
            if (_active.SetRate(rate) != 0) return false;
            _videoPlaybackRate = rate;
            return true;
        }
    }

    public IReadOnlyList<VideoSubtitleOption> GetVideoSubtitleOptions()
    {
        lock (_gate)
        {
            if (_disposed || !_videoTrack) return [];
            try
            {
                return _active.SpuDescription?
                    .Select(track => new VideoSubtitleOption(track.Id, string.IsNullOrWhiteSpace(track.Name) ? $"Subtitle {track.Id}" : track.Name))
                    .ToArray() ?? [];
            }
            catch (InvalidOperationException) { return []; }
        }
    }

    public bool SetVideoSubtitle(int trackId)
    {
        lock (_gate) return !_disposed && _videoTrack && _active.SetSpu(trackId);
    }

    public bool TakeVideoSnapshot(string path)
    {
        lock (_gate)
        {
            if (_disposed || !_videoTrack) return false;
            return _active.TakeSnapshot(0, path, 0, 0);
        }
    }

    public void SetVideoExtensions(IEnumerable<string> extensions)
    {
        lock (_gate)
            _videoExtensions = extensions.Select(extension => extension.StartsWith('.') ? extension : $".{extension}")
                .ToHashSet(StringComparer.OrdinalIgnoreCase);
    }

    public void SetVideoSurfaceHandle(nint handle)
    {
        lock (_gate)
        {
            if (_disposed) return;
            if (_videoTrack) _active.Hwnd = handle;
        }
    }

    private bool IsVideoPathCore(string path) => _videoExtensions.Contains(Path.GetExtension(path));

    private nint RequestVideoSurface(Track track)
    {
        Func<Track, nint>? request;
        lock (_gate)
        {
            if (_disposed || !IsVideoPathCore(track.Path)) return 0;
            request = VideoSurfaceRequested;
        }
        return request?.Invoke(track) ?? 0;
    }

    public void SelectAudioOutputDevice(string? deviceId)
    {
        lock (_gate)
        {
            if (_disposed) return;
            var resume = _playRequested;
            var position = Math.Max(_restorePosition, _active.Time);
            if (_independentMusicOutput && !_videoTrack) CancelCrossfadeCore();
            _audioOutputDeviceId = string.IsNullOrWhiteSpace(deviceId) ? null : deviceId;
            _pendingFallbackDeviceId = null;
            _directSoundDeviceId = _independentMusicOutput && _audioOutputDeviceId is not null
                ? AudioOutputRouting.DirectSoundDeviceId(_audioOutputDeviceId) : "";
            if (_independentMusicOutput && _audioOutputDeviceId is not null && _directSoundDeviceId.Length == 0)
                _audioOutputDeviceId = null;
            if (_independentMusicOutput && !_videoTrack && _activeMedia is not null)
            {
                // DirectSound selects the endpoint at stream creation. Reopen
                // this song at the same position when the user changes output.
                _mediaGeneration++;
                _active.Stop();
                Interlocked.Exchange(ref _subscriptions[_active].Generation, _mediaGeneration);
                ConfigureAudioOutputCore(_active);
                _restorePosition = Math.Max(0, position);
                _active.Volume = (int)_volume;
                if (resume) PlayLoaded();
            }
            else ApplyAudioOutputDevice(_active);
            if (_spare is not null) ApplyAudioOutputDevice(_spare);
        }
    }

    public void LoadPaused(Track track, long positionMilliseconds)
    {
        var requestedSurface = RequestVideoSurface(track);
        lock (_gate)
        {
            if (_disposed) return;
            _playRequested = false;
            CancelCrossfadeCore();
            _mediaGeneration++;
            _trackEnded = false;
            _active.Stop();
            _videoTrack = IsVideoPathCore(track.Path);
            if (_videoTrack && requestedSurface == 0)
            {
                _currentTrack = track;
                ReportPlaybackFailureCore(track);
                return;
            }
            if (_videoTrack)
            {
                _active.Hwnd = requestedSurface;
            }
            SetMedia(_active, ref _activeMedia, track.Path);
            ApplyAudioOutputDevice(_active);
            _active.Time = Math.Max(0, positionMilliseconds);
            _restorePosition = Math.Max(0, positionMilliseconds);
            _currentTrack = track;
            _reportedFailurePath = null;
        }
    }

    public void UpdateTrackMetadata(Track track)
    {
        lock (_gate)
            if (!_disposed && _currentTrack is { } current && string.Equals(current.Path, track.Path, StringComparison.OrdinalIgnoreCase))
                _currentTrack = track;
    }

    public void Play(Track track)
    {
        var requestedSurface = RequestVideoSurface(track);
        lock (_gate)
        {
            if (_disposed) return;
            CancelCrossfadeCore();
            _mediaGeneration++;
            _trackEnded = false;
            _active.Stop();
            _videoTrack = IsVideoPathCore(track.Path);
            if (_videoTrack && requestedSurface == 0)
            {
                _currentTrack = track;
                ReportPlaybackFailureCore(track);
                return;
            }
            if (_videoTrack)
            {
                _active.Hwnd = requestedSurface;
            }
            SetMedia(_active, ref _activeMedia, track.Path);
            ApplyAudioOutputDevice(_active);
            _restorePosition = 0;
            _currentTrack = track;
            _reportedFailurePath = null;
            _active.Volume = (int)_volume;
            _playRequested = true;
            if (!_active.Play()) ReportPlaybackFailureCore(track);
            _ = ReconcileStartVolumeAsync(_active, Interlocked.Read(ref _subscriptions[_active].Generation));
        }
    }

    public void PlayLoaded()
    {
        Track? resumeVideoTrack;
        Track? endedTrack;
        lock (_gate)
        {
            if (_disposed) return;
            resumeVideoTrack = _videoTrack ? _currentTrack : null;
            endedTrack = _trackEnded || _active.State == VLCState.Ended ? _currentTrack : null;
        }
        if (endedTrack is not null) LoadPaused(endedTrack, 0);
        var requestedSurface = resumeVideoTrack is null ? 0 : RequestVideoSurface(resumeVideoTrack);
        MediaPlayer player;
        long generation;
        long restorePosition;
        lock (_gate)
        {
            if (_disposed) return;
            if (_activeMedia is null) return;
            RefreshAudioRouteCore();
            if (_videoTrack)
            {
                if (requestedSurface == 0) { ReportPlaybackFailureCore(_currentTrack); return; }
                _active.Hwnd = requestedSurface;
            }
            ApplyAudioOutputDevice(_active);
            _reportedFailurePath = null;
            _playRequested = true;
            if (!_active.Play()) { ReportPlaybackFailureCore(_currentTrack); return; }
            player = _active;
            generation = _mediaGeneration;
            restorePosition = _restorePosition;
            _ = ReconcileStartVolumeAsync(player, generation);
        }
        if (restorePosition > 0) _ = SeekAfterStartAsync(player, generation, restorePosition);
    }

    public void Pause()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _playRequested = false;
            CancelCrossfadeCore();
            // Pause() toggles based on native playing state. A seek can briefly
            // enter buffering, so a pause request must set the desired state.
            _active.SetPause(true);
        }
    }

    public void Stop()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _playRequested = false;
            CancelCrossfadeCore();
            _mediaGeneration++;
            _restorePosition = 0;
            _active.Stop();
        }
    }

    public void CancelCrossfade()
    {
        lock (_gate)
            if (!_disposed) CancelCrossfadeCore();
    }

    public void Seek(long positionMilliseconds)
    {
        Track? reloadTrack = null;
        bool resume = false;
        lock (_gate)
        {
            if (_disposed) return;
            CancelCrossfadeCore();
            var state = _active.State;
            resume = _trackEnded || state == VLCState.Ended;
            var duration = Duration;
            positionMilliseconds = Math.Clamp(positionMilliseconds, 0, duration > 0 ? duration : long.MaxValue);
            if (resume || state is VLCState.NothingSpecial or VLCState.Stopped)
                reloadTrack = _currentTrack;
            if (reloadTrack is null)
            {
                _mediaGeneration++;
                Interlocked.Exchange(ref _subscriptions[_active].Generation, _mediaGeneration);
                _trackEnded = false;
                _restorePosition = 0;
                _active.Time = positionMilliseconds;
            }
        }

        if (reloadTrack is not null)
        {
            LoadPaused(reloadTrack, positionMilliseconds);
            if (resume) PlayLoaded();
        }
    }

    private async Task SeekAfterStartAsync(MediaPlayer player, long generation, long position)
    {
        try
        {
            for (var attempt = 0; attempt < 20; attempt++)
            {
                await Task.Delay(100).ConfigureAwait(false);
                lock (_gate)
                {
                    if (_disposed || generation != _mediaGeneration || !ReferenceEquals(player, _active)) return;
                    if (player.IsSeekable && player.State is VLCState.Playing or VLCState.Paused)
                    {
                        player.Time = player.Length > 0 ? Math.Min(position, player.Length) : position;
                        _restorePosition = 0;
                        return;
                    }
                }
            }
        }
        catch (ObjectDisposedException) { }
    }

    public void ApplyEqualizer(string? preset, IReadOnlyList<float>? bands = null, float? preamp = null)
    {
        lock (_gate)
        {
            if (_disposed) return;
            _equalizerPreset = string.IsNullOrWhiteSpace(preset) ? null : preset;
            _equalizerBands = bands?.ToArray();
            if (_equalizerPreset is not null)
            {
                using var labels = new Equalizer();
                using var selected = new Equalizer((uint)PresetIndex(_equalizerPreset, labels));
                _equalizerPreamp = selected.Preamp;
            }
            else if (preamp is { } gain && float.IsFinite(gain)) _equalizerPreamp = Math.Clamp(gain, -20, 20);
            else if (bands is null) _equalizerPreamp = NeutralEqualizerPreamp;
            ApplyEqualizerCore(_active);
            if (_spare is not null) ApplyEqualizerCore(_spare);
        }
    }

    public async Task CrossfadeToAsync(Track nextTrack, int milliseconds, CancellationToken cancellationToken = default)
    {
        bool involvesVideo;
        lock (_gate) involvesVideo = _videoTrack || IsVideoPathCore(nextTrack.Path);
        if (involvesVideo) { Play(nextTrack); return; }
        if (milliseconds <= 0) { Play(nextTrack); return; }
        cancellationToken.ThrowIfCancellationRequested();

        MediaPlayer old = _active;
        MediaPlayer incoming = _active;
        CancellationTokenSource? fade = null;
        var fadeMilliseconds = 0;
        try
        {
            lock (_gate)
            {
                if (_disposed) return;
                if (_currentTrack is not null)
                {
                    CancelCrossfadeCore();
                    old = _active;
                    var remaining = _active.Length > 0 ? Math.Max(0, _active.Length - _active.Time) : milliseconds;
                    fadeMilliseconds = Math.Min(milliseconds, (int)Math.Min(int.MaxValue, remaining));
                    if (fadeMilliseconds > 0)
                    {
                        incoming = EnsureSpareCore();
                        fade = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                        _crossfadeCancellation = fade;
                        _crossfadeTarget = nextTrack;
                        _crossfadeProgress = 0;
                        _mediaGeneration++;
                        incoming.Stop();
                        SetMedia(incoming, ref _spareMedia, nextTrack.Path);
                        ApplyAudioOutputDevice(incoming);
                        incoming.Volume = 0;
                        if (!incoming.Play()) throw new InvalidOperationException($"LibVLC refused to play '{nextTrack.Path}'. {_libVlc.LastLibVLCError}");
                    }
                }
            }
        }
        catch (Exception ex)
        {
            lock (_gate)
                if (!_disposed) CancelCrossfadeCore();
            LogCrossfadeFailure(nextTrack, ex);
            CrossfadeFailed?.Invoke(nextTrack);
            fade?.Dispose();
            return;
        }

        if (fadeMilliseconds <= 0) { Play(nextTrack); fade?.Dispose(); return; }
        var tokenSource = fade!;
        try
        {
            // Play() only schedules opening/decoding. Keep the outgoing song at
            // full volume until the incoming playback clock actually advances.
            var opening = System.Diagnostics.Stopwatch.StartNew();
            while (true)
            {
                await Task.Delay(20, tokenSource.Token).ConfigureAwait(false);
                lock (_gate)
                {
                    if (_disposed || !ReferenceEquals(_crossfadeCancellation, tokenSource) || tokenSource.IsCancellationRequested) return;
                    if (incoming.IsPlaying && incoming.Time > 0)
                    {
                        break;
                    }
                    if (incoming.State is VLCState.Error or VLCState.Ended || opening.Elapsed > TimeSpan.FromSeconds(5))
                        throw new InvalidOperationException("The incoming crossfade track did not become ready.");
                }
            }
            // The shell starts opening with a short lead. Hold the decoded track
            // at its beginning until the requested fade window actually starts.
            lock (_gate)
            {
                if (_disposed || !ReferenceEquals(_crossfadeCancellation, tokenSource)) return;
                incoming.SetPause(true);
                if (incoming.IsSeekable) incoming.Time = 0;
            }
            while (true)
            {
                await Task.Delay(20, tokenSource.Token).ConfigureAwait(false);
                lock (_gate)
                {
                    if (_disposed || !ReferenceEquals(_crossfadeCancellation, tokenSource)) return;
                    var remaining = old.Length > 0 ? Math.Max(0, old.Length - old.Time) : milliseconds;
                    if (remaining > milliseconds) continue;
                    incoming.SetPause(false);
                    break;
                }
            }
            opening.Restart();
            while (true)
            {
                await Task.Delay(20, tokenSource.Token).ConfigureAwait(false);
                lock (_gate)
                {
                    if (_disposed || !ReferenceEquals(_crossfadeCancellation, tokenSource)) return;
                    if (incoming.IsPlaying && incoming.Time > 0)
                    {
                        var remaining = old.Length > 0 ? Math.Max(0, old.Length - old.Time) : milliseconds;
                        // If opening consumed the entire tail, ramp the new stream in
                        // briefly; never stretch an overlap beyond the outgoing audio.
                        fadeMilliseconds = remaining > 0 ? Math.Min(milliseconds, (int)Math.Min(int.MaxValue, remaining)) : Math.Min(200, milliseconds);
                        break;
                    }
                    if (incoming.State is VLCState.Error or VLCState.Ended || opening.Elapsed > TimeSpan.FromSeconds(3))
                        throw new InvalidOperationException("The prepared crossfade track did not resume.");
                }
            }
            var start = System.Diagnostics.Stopwatch.GetTimestamp();
            while (true)
            {
                await Task.Delay(20, tokenSource.Token).ConfigureAwait(false);
                Track? completed = null;
                lock (_gate)
                {
                    if (_disposed || !ReferenceEquals(_crossfadeCancellation, tokenSource) || tokenSource.IsCancellationRequested) return;
                    var elapsed = System.Diagnostics.Stopwatch.GetElapsedTime(start).TotalMilliseconds;
                    _crossfadeProgress = fadeMilliseconds > 0 ? Math.Clamp(elapsed / fadeMilliseconds, 0, 1) : 1;
                    ApplyCrossfadeVolumeCore();
                    if (_crossfadeProgress >= 1)
                    {
                        old.Stop();
                        _active = incoming;
                        _spare = old;
                        (_activeMedia, _spareMedia) = (_spareMedia, _activeMedia);
                        _currentTrack = nextTrack;
                        _crossfadeCommittedGeneration = _mediaGeneration;
                        _trackEnded = false;
                        _restorePosition = 0;
                        _active.Volume = (int)_volume;
                        _spare.Volume = 0;
                        _crossfadeTarget = null;
                        _crossfadeCancellation = null;
                        completed = nextTrack;
                    }
                }
                if (completed is not null)
                {
                    CrossfadeCompleted?.Invoke(completed);
                    return;
                }
            }
        }
        catch (OperationCanceledException) when (tokenSource.IsCancellationRequested) { }
        catch (Exception ex)
        {
            lock (_gate)
                if (ReferenceEquals(_crossfadeCancellation, tokenSource)) CancelCrossfadeCore();
            LogCrossfadeFailure(nextTrack, ex);
            CrossfadeFailed?.Invoke(nextTrack);
        }
        finally
        {
            lock (_gate)
                if (ReferenceEquals(_crossfadeCancellation, tokenSource)) CancelCrossfadeCore();
            tokenSource.Dispose();
        }
    }

    public static IReadOnlyList<string> EqualizerPresets()
    {
        using var equalizer = new Equalizer();
        return Enumerable.Range(0, (int)equalizer.PresetCount).Select(i => equalizer.PresetName((uint)i) ?? $"Preset {i + 1}").ToArray();
    }

    public static IReadOnlyList<string> EqualizerBands()
    {
        using var equalizer = new Equalizer();
        return Enumerable.Range(0, (int)equalizer.BandCount).Select(i => equalizer.BandFrequency((uint)i) >= 1000
            ? $"{equalizer.BandFrequency((uint)i) / 1000:0.#} kHz" : $"{equalizer.BandFrequency((uint)i):0} Hz").ToArray();
    }

    public static IReadOnlyList<float> EqualizerPresetBands(string name)
    {
        using var equalizer = new Equalizer(); var presetIndex = PresetIndex(name, equalizer);
        using var preset = new Equalizer((uint)presetIndex);
        return Enumerable.Range(0, (int)preset.BandCount).Select(i => preset.Amp((uint)i)).ToArray();
    }

    private static int PresetIndex(string name, Equalizer equalizer)
    {
        for (var i = 0; i < equalizer.PresetCount; i++) if (string.Equals(equalizer.PresetName((uint)i), name, StringComparison.OrdinalIgnoreCase)) return i;
        return 0;
    }

    private MediaPlayer EnsureSpareCore()
    {
        if (_spare is not null) return _spare;
        _spare = new MediaPlayer(_libVlc);
        Subscribe(_spare);
        _spare.Volume = (int)_volume;
        ApplyAudioOutputDevice(_spare);
        ApplyEqualizerCore(_spare);
        return _spare;
    }

    private void ApplyEqualizerCore(MediaPlayer player)
    {
        // Keep a neutral filter enabled from stream creation. Changing Off to a
        // curve then updates coefficients without rebuilding the audio pipeline.
        // VLC's one-pass filter attenuates dry input by 0.25, requiring +12.0412dB.
        using var current = new Equalizer();
        using var equalizer = _equalizerPreset is not null
            ? new Equalizer((uint)Math.Clamp(PresetIndex(_equalizerPreset, current), 0, (int)current.PresetCount - 1))
            : new Equalizer();
        if (_equalizerBands is not null)
            for (var i = 0; i < Math.Min(_equalizerBands.Count, (int)equalizer.BandCount); i++) equalizer.SetAmp(_equalizerBands[i], (uint)i);
        equalizer.SetPreamp(_equalizerPreamp);
        player.SetEqualizer(equalizer);
    }

    private void SetMedia(MediaPlayer player, ref Media? media, string path)
    {
        RefreshAudioRouteCore();
        ConfigureAudioOutputCore(player);
        media?.Dispose();
        if (_fileInputs.Remove(player, out var previousInput)) previousInput.Dispose();
        var extension = Path.GetExtension(path).ToLowerInvariant();
        var dffDuration = extension == ".dff" ? (long)DffAudio.ReadDuration(path).TotalMilliseconds : 0;
        if (dffDuration > 0)
        {
            var input = new DffStreamInput(new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete), dffDuration);
            try { media = new Media(_libVlc, input); _fileInputs.Add(player, input); }
            catch { input.Dispose(); throw; }
        }
        else media = new Media(_libVlc, new Uri(path));
        // The native Ogg reader can report EOF immediately after a time seek.
        // Use VLC's bundled FFmpeg reader, retaining the same VLC decoders.
        if (Path.GetExtension(path).ToLowerInvariant() is ".ogg" or ".oga")
            media.AddOption(":demux=avformat");
        if (ReferenceEquals(player, _active) && _videoTrack)
        {
            foreach (var subtitlePath in FindNearbySubtitleFiles(path))
            {
                try
                {
                    if (!media.AddSlave(MediaSlaveType.Subtitle, 4, new Uri(subtitlePath)))
                        LocalAppLog.Shared.Warning("video-subtitle", $"LibVLC could not add the nearby subtitle '{Path.GetFileName(subtitlePath)}'.");
                }
                catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException)
                {
                    LocalAppLog.Shared.Warning("video-subtitle", $"Could not add the nearby subtitle '{Path.GetFileName(subtitlePath)}'.", ex);
                }
            }
        }
        var requestedRate = ReferenceEquals(player, _active) && _videoTrack ? _videoPlaybackRate : 1f;
        // Stop has released callbacks for the previous media. Bind subsequent
        // native events to this media, including events from the spare player.
        Interlocked.Exchange(ref _subscriptions[player].Generation, _mediaGeneration);
        player.Media = media;
        if (player.SetRate(requestedRate) != 0 && requestedRate != 1f)
            LocalAppLog.Shared.Warning("video-playback", "The media engine rejected the requested video playback speed.");
    }

    private static string[] FindNearbySubtitleFiles(string videoPath)
    {
        var directory = Path.GetDirectoryName(videoPath);
        var mediaName = Path.GetFileNameWithoutExtension(videoPath);
        if (string.IsNullOrWhiteSpace(directory) || string.IsNullOrWhiteSpace(mediaName) || !Directory.Exists(directory)) return [];
        var subtitleExtensions = new HashSet<string>([".srt", ".ass", ".ssa", ".vtt"], StringComparer.OrdinalIgnoreCase);
        try
        {
            return Directory.EnumerateFiles(directory)
                .Where(candidate =>
                {
                    var extension = Path.GetExtension(candidate);
                    if (!subtitleExtensions.Contains(extension)) return false;
                    var subtitleName = Path.GetFileNameWithoutExtension(candidate);
                    return subtitleName.Equals(mediaName, StringComparison.OrdinalIgnoreCase) ||
                        subtitleName.StartsWith(mediaName + ".", StringComparison.OrdinalIgnoreCase);
                })
                .OrderBy(candidate => Path.GetFileNameWithoutExtension(candidate).Equals(mediaName, StringComparison.OrdinalIgnoreCase) ? 0 : 1)
                .ThenBy(candidate => candidate, StringComparer.OrdinalIgnoreCase)
                .Take(8)
                .ToArray();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            LocalAppLog.Shared.Warning("video-subtitle", "Could not look for subtitle files next to the video.", ex);
            return [];
        }
    }

    private void ApplyAudioOutputDevice(MediaPlayer player)
    {
        if (_independentMusicOutput && !(ReferenceEquals(player, _active) && _videoTrack))
        {
            // The module shortcut is directsound; its device option is directx.
            player.SetOutputDevice(_directSoundDeviceId, "directx");
            return;
        }
        var deviceId = AudioOutputRouting.EndpointId(_audioOutputDeviceId ?? string.Empty);
        player.SetOutputDevice(deviceId, "mmdevice");
        player.SetOutputDevice(deviceId);
    }

    private void ConfigureAudioOutputCore(MediaPlayer player)
    {
        if (!_independentMusicOutput) return;
        var video = ReferenceEquals(player, _active) && _videoTrack;
        var route = (Output: video ? "mmdevice" : "directsound", Device: video
            ? AudioOutputRouting.EndpointId(_audioOutputDeviceId ?? "") : _directSoundDeviceId);
        if (_audioRoutes.TryGetValue(player, out var previous) && previous == route) return;
        // Configure the option before VLC creates the output and inherits it.
        player.SetOutputDevice(route.Device, video ? "mmdevice" : "directx");
        if (!player.SetAudioOutput(route.Output)) throw new InvalidOperationException("LibVLC could not configure the audio output.");
        _audioRoutes[player] = route;
        ApplyEqualizerCore(player);
        player.Volume = (int)_volume;
    }

    internal static (int Outgoing, int Incoming) CrossfadeVolumes(int volume, double progress)
    {
        volume = Math.Clamp(volume, 0, 100);
        progress = Math.Clamp(progress, 0, 1);
        if (progress <= 0) return (volume, 0);
        if (progress >= 1) return (0, volume);
        // VLC's Windows volume scale cubes the slider value. Convert equal-power
        // amplitude gains back into that scale to avoid a deep midpoint dip.
        var angle = progress * Math.PI / 2;
        return ((int)Math.Round(volume * Math.Cbrt(Math.Cos(angle))),
            (int)Math.Round(volume * Math.Cbrt(Math.Sin(angle))));
    }

    private void ApplyCrossfadeVolumeCore()
    {
        var volumes = CrossfadeVolumes((int)_volume, _crossfadeCancellation is null ? 0 : _crossfadeProgress);
        _active.Volume = volumes.Outgoing;
        if (_spare is not null) _spare.Volume = volumes.Incoming;
    }

    private void Subscribe(MediaPlayer player)
    {
        // LibVLCSharp sends its MediaPlayerEventManager as sender, not the player.
        // Capture the owner explicitly and leave native callbacks immediately:
        // Stop/Dispose can wait for them while another thread holds _gate.
        var handlers = new PlayerSubscription();
        handlers.End = (_, _) => QueuePlayerEvent(player, Interlocked.Read(ref handlers.Generation), endReached: true);
        handlers.Error = (_, _) => QueuePlayerEvent(player, Interlocked.Read(ref handlers.Generation), endReached: false);
        handlers.Playing = (_, _) =>
        {
            var generation = Interlocked.Read(ref handlers.Generation);
            ThreadPool.QueueUserWorkItem(_ => { _ = ReconcileStartVolumeAsync(player, generation); });
        };
        _subscriptions.Add(player, handlers);
        player.EndReached += handlers.End;
        player.EncounteredError += handlers.Error;
        player.Playing += handlers.Playing;
    }

    private void Unsubscribe(MediaPlayer player)
    {
        if (!_subscriptions.Remove(player, out var handlers)) return;
        player.EndReached -= handlers.End;
        player.EncounteredError -= handlers.Error;
        player.Playing -= handlers.Playing;
    }

    private async Task ReconcileStartVolumeAsync(MediaPlayer player, long generation)
    {
        // Setting Volume before Play can fail because VLC has no audio output yet.
        // Native callbacks only schedule this worker; never call VLC from a callback.
        PlayerSubscription owner;
        lock (_gate)
        {
            if (_disposed || !_subscriptions.TryGetValue(player, out owner!) || owner.Generation != generation || owner.VolumeWorkerGeneration == generation) return;
            owner.VolumeWorkerGeneration = generation;
        }
        try
        {
            for (var attempt = 0; attempt < 100; attempt++)
            {
                await Task.Delay(25).ConfigureAwait(false);
                lock (_gate)
                {
                    if (_disposed || !_subscriptions.TryGetValue(player, out var subscription) ||
                        Interlocked.Read(ref subscription.Generation) != generation ||
                        (!ReferenceEquals(player, _active) && (_crossfadeCancellation is null || !ReferenceEquals(player, _spare)))) return;
                    var volumes = CrossfadeVolumes((int)_volume, _crossfadeCancellation is null ? 0 : _crossfadeProgress);
                    var desired = ReferenceEquals(player, _active) ? volumes.Outgoing : volumes.Incoming;
                    if (player.Volume != desired) player.Volume = desired;
                    if (attempt >= 3 && player.IsPlaying && player.Time > 0 && player.Volume == desired) return;
                }
            }
        }
        catch (ObjectDisposedException) { }
        finally
        {
            lock (_gate)
                if (owner.VolumeWorkerGeneration == generation) owner.VolumeWorkerGeneration = -1;
        }
    }

    private void RefreshAudioRouteCore()
    {
        if (!_independentMusicOutput || _audioOutputDeviceId is null) return;
        var mapped = AudioOutputRouting.DirectSoundDeviceId(_audioOutputDeviceId);
        _directSoundDeviceId = mapped;
        if (mapped.Length > 0) return;
        QueueAudioOutputFallbackCore();
    }

    private void QueueAudioOutputFallbackCore()
    {
        if (_audioOutputDeviceId is null || _pendingFallbackDeviceId == _audioOutputDeviceId) return;
        _pendingFallbackDeviceId = _audioOutputDeviceId;
        ThreadPool.QueueUserWorkItem(_ => AudioOutputFallback?.Invoke(this, EventArgs.Empty));
    }

    private void CheckAudioOutput()
    {
        if (Interlocked.Exchange(ref _checkingOutput, 1) != 0) return;
        try
        {
            string? deviceId;
            lock (_gate) deviceId = !_disposed && _independentMusicOutput ? _audioOutputDeviceId : null;
            if (deviceId is null || AudioOutputRouting.DirectSoundDeviceId(deviceId).Length > 0) return;
            lock (_gate)
            {
                if (!_disposed && _audioOutputDeviceId == deviceId) QueueAudioOutputFallbackCore();
            }
        }
        catch (Exception ex) when (ex is InvalidOperationException or System.Runtime.InteropServices.COMException)
        { LocalAppLog.Shared.Warning("audio-devices", "Could not refresh the audio output route.", ex); }
        finally { Volatile.Write(ref _checkingOutput, 0); }
    }

    private void QueuePlayerEvent(MediaPlayer player, long generation, bool endReached)
    {
        ThreadPool.QueueUserWorkItem(_ =>
        {
            if (endReached) EndReached(player, generation);
            else EncounteredError(player, generation);
        });
    }

    private void EndReached(MediaPlayer player, long generation)
    {
        bool ended;
        string reason;
        lock (_gate)
        {
            var senderIsActive = ReferenceEquals(player, _active);
            var hasCurrentTrack = _currentTrack is not null;
            var crossfadePending = _crossfadeCancellation is not null;
            var duplicate = _handledEndReachedGeneration == _mediaGeneration;
            var stale = generation != _mediaGeneration;
            ended = !_disposed && !stale && !duplicate && PlaybackEventPolicy.ShouldHandleEndReached(
                senderIsActive, hasCurrentTrack, currentStateIsEnded: true, crossfadePending);
            reason = _disposed ? "service disposed"
                : stale ? "media changed before event was handled"
                : duplicate ? "duplicate event for current media"
                : !senderIsActive ? "event came from inactive player"
                : !hasCurrentTrack ? "no current track"
                : crossfadePending ? "crossfade is already advancing the queue"
                : "accepted";

            if (ended)
            {
                _playRequested = false;
                _handledEndReachedGeneration = _mediaGeneration;
                _trackEnded = true;
            }
        }

        LocalAppLog.Shared.Info("playback", $"LibVLC end event {(ended ? "accepted" : "ignored")}: {reason}.");
        // Native callback has been released; the shell posts queue work to its dispatcher.
        if (ended) TrackEnded?.Invoke(this, EventArgs.Empty);
    }

    private void EncounteredError(MediaPlayer player, long generation)
    {
        CheckAudioOutput();
        Track? activeFailure = null;
        Track? fadeFailure = null;
        lock (_gate)
        {
            if (_disposed || generation != _mediaGeneration) return;
            // The native error event is authoritative. LibVLC may already be
            // Stopped by the time this worker runs; querying State loses errors.
            if (PlaybackEventPolicy.ShouldHandleActiveError(
                    ReferenceEquals(player, _active),
                    _currentTrack is not null,
                    currentStateIsError: true))
            {
                if (_currentTrack is { } current && !string.Equals(_reportedFailurePath, current.Path, StringComparison.OrdinalIgnoreCase))
                {
                    _reportedFailurePath = current.Path;
                    if (_pendingFallbackDeviceId is null) _playRequested = false;
                    activeFailure = current;
                }
            }
            else if (PlaybackEventPolicy.ShouldHandleCrossfadeError(
                         ReferenceEquals(player, _spare),
                         _crossfadeTarget is not null,
                         currentStateIsError: true))
            {
                fadeFailure = _crossfadeTarget;
                CancelCrossfadeCore();
            }
        }
        if (activeFailure is not null) ReportPlaybackFailure(activeFailure);
        if (fadeFailure is not null)
        {
            LogCrossfadeFailure(fadeFailure, new InvalidOperationException(_libVlc.LastLibVLCError));
            CrossfadeFailed?.Invoke(fadeFailure);
        }
    }

    private void ReportPlaybackFailureCore(Track? track)
    {
        if (track is null || string.Equals(_reportedFailurePath, track.Path, StringComparison.OrdinalIgnoreCase)) return;
        if (_pendingFallbackDeviceId is null) _playRequested = false;
        _reportedFailurePath = track.Path;
        LocalAppLog.Shared.Warning("playback", $"LibVLC could not open or decode '{track.Path}'. {_libVlc.LastLibVLCError}");
        PlaybackFailed?.Invoke(track);
    }

    private void ReportPlaybackFailure(Track track)
    {
        LocalAppLog.Shared.Warning("playback", $"LibVLC could not open or decode '{track.Path}'. {_libVlc.LastLibVLCError}");
        PlaybackFailed?.Invoke(track);
    }

    private static void LogCrossfadeFailure(Track track, Exception exception)
    {
        LocalAppLog.Shared.Error("crossfade", $"Could not crossfade to '{track.Path}'.", exception);
    }

    private void CancelCrossfadeCore()
    {
        var fade = _crossfadeCancellation;
        _crossfadeCancellation = null;
        _crossfadeTarget = null;
        _crossfadeProgress = 0;
        fade?.Cancel();
        if (_disposed) return;
        if (_spare is not null)
        {
            _spare.Stop();
            _spare.Volume = 0;
        }
        // A canceled/failed fade keeps the outgoing media alive. Its eventual
        // end/error belongs to the current generation again.
        if (fade is not null)
        {
            Interlocked.Exchange(ref _subscriptions[_active].Generation, _mediaGeneration);
            // Its native end may have arrived while the fade suppressed it.
            if (_active.State == VLCState.Ended) QueuePlayerEvent(_active, _mediaGeneration, endReached: true);
        }
        _active.Volume = (int)_volume;
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            CancelCrossfadeCore();
            _disposed = true;
            _outputMonitor.Dispose();
            _active.Stop();
            Unsubscribe(_active);
            _activeMedia?.Dispose();
            _spareMedia?.Dispose();
            _active.Dispose();
            if (_spare is not null)
            {
                Unsubscribe(_spare);
                _spare.Dispose();
            }
            _libVlc.Dispose();
            foreach (var input in _fileInputs.Values) input.Dispose();
            _fileInputs.Clear();
        }
    }
}
