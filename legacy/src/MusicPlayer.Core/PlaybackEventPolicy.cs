namespace MusicPlayer.Core;

/// <summary>Rejects stale LibVLC callbacks that arrive while a reused player is changing media.</summary>
public static class PlaybackEventPolicy
{
    public static bool ShouldHandleEndReached(bool senderIsActivePlayer, bool hasCurrentTrack, bool currentStateIsEnded, bool crossfadePending) =>
        senderIsActivePlayer && hasCurrentTrack && currentStateIsEnded && !crossfadePending;

    public static bool ShouldHandleActiveError(bool senderIsActivePlayer, bool hasCurrentTrack, bool currentStateIsError) =>
        senderIsActivePlayer && hasCurrentTrack && currentStateIsError;

    public static bool ShouldHandleCrossfadeError(bool senderIsCrossfadePlayer, bool hasCrossfadeTarget, bool currentStateIsError) =>
        senderIsCrossfadePlayer && hasCrossfadeTarget && currentStateIsError;
}
