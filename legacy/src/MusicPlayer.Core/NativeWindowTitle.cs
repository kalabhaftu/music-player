namespace MusicPlayer.Core;

public static class NativeWindowTitle
{
    public static string ForTrack(Track? track) => track is null ? "Music Player" :
        string.IsNullOrWhiteSpace(track.Artist) ? $"{track.Title} | Music Player" : $"{track.Title} — {track.Artist} | Music Player";
}
