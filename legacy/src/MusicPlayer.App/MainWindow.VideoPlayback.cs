using System.Runtime.InteropServices;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private VideoPlaybackWindow? _videoWindow;

    private nint RequestVideoPlaybackSurface(Track track)
    {
        if (_windowClosed) return 0;
        try
        {
            if (_videoWindow is { } existing)
            {
                existing.SetTrack(track);
                existing.Activate();
                return existing.SurfaceHandle;
            }

            var created = new VideoPlaybackWindow(track, _playback,
                togglePlayback: () => { if (_playback.IsPlaying) PausePlayback(); else ResumePlayback(); },
                previous: PlayPreviousTrack,
                next: PlayNextTrack,
                seek: milliseconds =>
                {
                    CancelCrossfadeAndRestoreQueue();
                    _playback.Seek(milliseconds);
                    PublishPlaybackState();
                },
                setVolume: SetPlaybackVolume,
                notify: message => _ = ShowNoticeAsync(message),
                closedCallback: HandleVideoPlaybackWindowClosed);
            _videoWindow = created;
            created.ApplyTheme(ShellRoot.RequestedTheme, ShellRoot.ActualTheme, ActiveProductIconPath());
            created.Activate();
            return created.SurfaceHandle;
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or UnauthorizedAccessException)
        {
            LocalAppLog.Shared.Error("video-window", "Could not create the video playback window.", ex);
            _ = ShowNoticeAsync("Music Player could not open the video window. See the local log for details.", Microsoft.UI.Xaml.Controls.InfoBarSeverity.Error);
            return 0;
        }
    }

    private void HandleVideoPlaybackWindowClosed(VideoPlaybackWindow window, bool userClosed)
    {
        if (!ReferenceEquals(_videoWindow, window)) return;
        _videoWindow = null;
        try
        {
            if (userClosed && _playback.IsVideoMode) PausePlayback();
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Warning("video-window", "Could not pause video when its window closed.", ex);
        }
        finally
        {
            try { _playback.SetVideoSurfaceHandle(0); }
            catch (Exception ex) { LocalAppLog.Shared.Warning("video-window", "Could not detach the closing video surface.", ex); }
        }
    }

    private void CloseVideoPlaybackWindow(bool pauseVideo)
    {
        var window = _videoWindow;
        if (window is null) return;
        _videoWindow = null;
        try
        {
            if (pauseVideo && _playback.IsVideoMode) PausePlayback();
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Warning("video-window", "Could not pause video before its owner closed the video window.", ex);
        }
        finally
        {
            try { _playback.SetVideoSurfaceHandle(0); }
            catch (Exception ex) { LocalAppLog.Shared.Warning("video-window", "Could not detach the video surface before closing its window.", ex); }
            window.CloseFromOwner();
        }
    }
}
