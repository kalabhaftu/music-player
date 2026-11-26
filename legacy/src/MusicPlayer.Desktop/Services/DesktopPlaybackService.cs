using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using LibVLCSharp.Shared;
using MusicPlayer.Desktop.Models;

namespace MusicPlayer.Desktop.Services;

public enum RepeatMode
{
    Off,
    All,
    One
}

public sealed class DesktopPlaybackService : IDisposable
{
    private readonly List<UiTrack> _queue = [];
    private readonly List<int> _shuffledIndices = [];
    private int _currentIndex = -1;
    private bool _isPlaying;
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 75;
    private bool _isMuted;
    private bool _shuffle;
    private RepeatMode _repeat = RepeatMode.Off;

    private readonly LibVLC? _libVlc;
    private readonly MediaPlayer? _mediaPlayer;
    private Media? _currentMedia;
    private readonly bool _vlcInitialized;
    private readonly System.Timers.Timer? _progressTimer;

    public event Action<UiTrack?>? TrackChanged;
    public event Action<bool>? PlaybackStateChanged;
    public event Action<double, double>? PositionChanged;
    public event Action? QueueChanged;
    public event Action<int>? VolumeChanged;

    public DesktopPlaybackService()
    {
        try
        {
            LibVLCSharp.Shared.Core.Initialize();
            _libVlc = new LibVLC("--no-video");
            _mediaPlayer = new MediaPlayer(_libVlc);
            _mediaPlayer.EndReached += (s, e) => Avalonia.Threading.Dispatcher.UIThread.Post(Next);
            _mediaPlayer.TimeChanged += (s, e) =>
            {
                _positionSeconds = e.Time / 1000.0;
                Avalonia.Threading.Dispatcher.UIThread.Post(() => PositionChanged?.Invoke(_positionSeconds, _durationSeconds));
            };
            _mediaPlayer.LengthChanged += (s, e) =>
            {
                if (e.Length > 0)
                {
                    _durationSeconds = e.Length / 1000.0;
                    Avalonia.Threading.Dispatcher.UIThread.Post(() => PositionChanged?.Invoke(_positionSeconds, _durationSeconds));
                }
            };
            _vlcInitialized = true;
        }
        catch
        {
            _vlcInitialized = false;
            _progressTimer = new System.Timers.Timer(250);
            _progressTimer.Elapsed += OnTimerTick;
        }
    }

    public UiTrack? CurrentTrack => _currentIndex >= 0 && _currentIndex < _queue.Count 
        ? _queue[_currentIndex] 
        : null;

    public IReadOnlyList<UiTrack> Queue => _queue;
    public int CurrentIndex => _currentIndex;
    public bool IsPlaying => _isPlaying;
    public double PositionSeconds => _positionSeconds;
    public double DurationSeconds => _durationSeconds;
    public int Volume => _volume;
    public bool IsMuted => _isMuted;
    public bool Shuffle
    {
        get => _shuffle;
        set
        {
            if (_shuffle == value) return;
            _shuffle = value;
            RebuildShuffleIndices();
        }
    }
    public RepeatMode Repeat
    {
        get => _repeat;
        set => _repeat = value;
    }

    public void PlayTrack(UiTrack track, IEnumerable<UiTrack>? newQueue = null)
    {
        if (newQueue != null)
        {
            _queue.Clear();
            _queue.AddRange(newQueue);
            RebuildShuffleIndices();
            QueueChanged?.Invoke();
        }

        var idx = _queue.FindIndex(t => t.Path.Equals(track.Path, StringComparison.OrdinalIgnoreCase));
        if (idx == -1)
        {
            _queue.Insert(0, track);
            _currentIndex = 0;
            RebuildShuffleIndices();
            QueueChanged?.Invoke();
        }
        else
        {
            _currentIndex = idx;
        }

        StartPlayback();
    }

    public void PlayQueueIndex(int index)
    {
        if (index < 0 || index >= _queue.Count) return;
        _currentIndex = index;
        StartPlayback();
    }

    public void TogglePlayPause()
    {
        if (CurrentTrack == null)
        {
            if (_queue.Count > 0)
            {
                _currentIndex = 0;
                StartPlayback();
            }
            return;
        }

        if (_isPlaying)
        {
            Pause();
        }
        else
        {
            Resume();
        }
    }

    public void Pause()
    {
        _isPlaying = false;
        if (_vlcInitialized && _mediaPlayer != null)
        {
            _mediaPlayer.Pause();
        }
        else
        {
            _progressTimer?.Stop();
        }
        PlaybackStateChanged?.Invoke(false);
    }

    public void Resume()
    {
        if (CurrentTrack == null) return;
        _isPlaying = true;
        if (_vlcInitialized && _mediaPlayer != null)
        {
            _mediaPlayer.Play();
        }
        else
        {
            _progressTimer?.Start();
        }
        PlaybackStateChanged?.Invoke(true);
    }

    public void Next()
    {
        if (_queue.Count == 0) return;

        if (_repeat == RepeatMode.One)
        {
            _positionSeconds = 0;
            StartPlayback();
            return;
        }

        if (_shuffle && _shuffledIndices.Count == _queue.Count)
        {
            var currentPos = _shuffledIndices.IndexOf(_currentIndex);
            if (currentPos + 1 < _shuffledIndices.Count)
            {
                _currentIndex = _shuffledIndices[currentPos + 1];
                StartPlayback();
                return;
            }
            else if (_repeat == RepeatMode.All)
            {
                _currentIndex = _shuffledIndices[0];
                StartPlayback();
                return;
            }
        }
        else
        {
            if (_currentIndex + 1 < _queue.Count)
            {
                _currentIndex++;
                StartPlayback();
                return;
            }
            else if (_repeat == RepeatMode.All)
            {
                _currentIndex = 0;
                StartPlayback();
                return;
            }
        }

        // End of queue reached and repeat off
        Pause();
        _positionSeconds = _durationSeconds;
        PositionChanged?.Invoke(_positionSeconds, _durationSeconds);
    }

    public void Previous()
    {
        if (_queue.Count == 0) return;

        if (_positionSeconds > 3)
        {
            Seek(0);
            return;
        }

        if (_shuffle && _shuffledIndices.Count == _queue.Count)
        {
            var currentPos = _shuffledIndices.IndexOf(_currentIndex);
            if (currentPos > 0)
            {
                _currentIndex = _shuffledIndices[currentPos - 1];
                StartPlayback();
                return;
            }
            else if (_repeat == RepeatMode.All)
            {
                _currentIndex = _shuffledIndices[^1];
                StartPlayback();
                return;
            }
        }
        else
        {
            if (_currentIndex > 0)
            {
                _currentIndex--;
                StartPlayback();
                return;
            }
            else if (_repeat == RepeatMode.All)
            {
                _currentIndex = _queue.Count - 1;
                StartPlayback();
                return;
            }
        }

        Seek(0);
    }

    public void Seek(double seconds)
    {
        if (seconds < 0) seconds = 0;
        if (seconds > _durationSeconds && _durationSeconds > 0) seconds = _durationSeconds;
        _positionSeconds = seconds;
        if (_vlcInitialized && _mediaPlayer != null)
        {
            _mediaPlayer.Time = (long)(seconds * 1000);
        }
        PositionChanged?.Invoke(_positionSeconds, _durationSeconds);
    }

    public void SetVolume(int volume)
    {
        _volume = Math.Clamp(volume, 0, 100);
        if (_volume > 0 && _isMuted) _isMuted = false;
        if (_vlcInitialized && _mediaPlayer != null)
        {
            _mediaPlayer.Volume = _isMuted ? 0 : _volume;
        }
        VolumeChanged?.Invoke(_isMuted ? 0 : _volume);
    }

    public void ToggleMute()
    {
        _isMuted = !_isMuted;
        if (_vlcInitialized && _mediaPlayer != null)
        {
            _mediaPlayer.Volume = _isMuted ? 0 : _volume;
        }
        VolumeChanged?.Invoke(_isMuted ? 0 : _volume);
    }

    public void Enqueue(UiTrack track)
    {
        _queue.Add(track);
        RebuildShuffleIndices();
        QueueChanged?.Invoke();
    }

    public void EnqueueNext(UiTrack track)
    {
        if (_currentIndex >= 0 && _currentIndex < _queue.Count)
        {
            _queue.Insert(_currentIndex + 1, track);
        }
        else
        {
            _queue.Add(track);
        }
        RebuildShuffleIndices();
        QueueChanged?.Invoke();
    }

    public void RemoveFromQueue(int index)
    {
        if (index < 0 || index >= _queue.Count) return;
        _queue.RemoveAt(index);
        if (index < _currentIndex) _currentIndex--;
        else if (index == _currentIndex)
        {
            if (_queue.Count == 0)
            {
                _currentIndex = -1;
                Pause();
                TrackChanged?.Invoke(null);
            }
            else
            {
                _currentIndex = Math.Min(_currentIndex, _queue.Count - 1);
                StartPlayback();
            }
        }
        RebuildShuffleIndices();
        QueueChanged?.Invoke();
    }

    public void ClearQueue()
    {
        _queue.Clear();
        _currentIndex = -1;
        Pause();
        TrackChanged?.Invoke(null);
        QueueChanged?.Invoke();
    }

    private void StartPlayback()
    {
        var track = CurrentTrack;
        if (track == null) return;

        _durationSeconds = track.Duration.TotalSeconds;
        _positionSeconds = 0;
        _isPlaying = true;

        if (_vlcInitialized && _mediaPlayer != null && _libVlc != null && File.Exists(track.Path))
        {
            _currentMedia?.Dispose();
            _currentMedia = new Media(_libVlc, new Uri(track.Path));
            _mediaPlayer.Play(_currentMedia);
            _mediaPlayer.Volume = _isMuted ? 0 : _volume;
        }
        else
        {
            _progressTimer?.Start();
        }

        TrackChanged?.Invoke(track);
        PlaybackStateChanged?.Invoke(true);
        PositionChanged?.Invoke(0, _durationSeconds);
    }

    private void OnTimerTick(object? sender, System.Timers.ElapsedEventArgs e)
    {
        if (!_isPlaying) return;

        _positionSeconds += 0.25;
        if (_positionSeconds >= _durationSeconds && _durationSeconds > 0)
        {
            Next();
        }
        else
        {
            PositionChanged?.Invoke(_positionSeconds, _durationSeconds);
        }
    }

    private void RebuildShuffleIndices()
    {
        _shuffledIndices.Clear();
        if (_queue.Count == 0) return;
        _shuffledIndices.AddRange(Enumerable.Range(0, _queue.Count));
        var rng = new Random();
        for (int i = _shuffledIndices.Count - 1; i > 0; i--)
        {
            int j = rng.Next(i + 1);
            (_shuffledIndices[i], _shuffledIndices[j]) = (_shuffledIndices[j], _shuffledIndices[i]);
        }
    }

    public void Dispose()
    {
        _currentMedia?.Dispose();
        _mediaPlayer?.Dispose();
        _libVlc?.Dispose();
        _progressTimer?.Dispose();
    }
}
