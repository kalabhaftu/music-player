using System.Net;
using System.Text.Json;

namespace MusicPlayer.Core;

public sealed class LyricsRateLimitException(TimeSpan retryAfter)
    : HttpRequestException("LRCLIB is limiting requests. Please try again shortly.", null, HttpStatusCode.TooManyRequests)
{
    public TimeSpan RetryAfter { get; } = retryAfter;
}

/// <summary>Serializes catalog requests; only a real HTTP 429 imposes a retry cooldown.</summary>
public sealed class LrclibSearchClient(HttpClient client, TimeProvider? timeProvider = null, TimeSpan? requestInterval = null)
{
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly TimeProvider _clock = timeProvider ?? TimeProvider.System;
    private readonly TimeSpan _interval = requestInterval ?? TimeSpan.FromMilliseconds(1300);
    private DateTimeOffset _nextRequest;
    private DateTimeOffset _backoffUntil;

    public async Task<IReadOnlyList<LyricsSearchResult>> SearchAsync(string title, string artist, CancellationToken cancellationToken = default)
    {
        // Bound the complete operation, including a queued request and one transient retry.
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(TimeSpan.FromSeconds(100));
        var token = deadline.Token;
        await _gate.WaitAsync(token).ConfigureAwait(false);
        try
        {
            var url = "https://lrclib.net/api/search?track_name=" + Uri.EscapeDataString(title)
                + "&artist_name=" + Uri.EscapeDataString(artist);
            for (var attempt = 0; ; attempt++)
            {
                var now = _clock.GetUtcNow();
                if (_backoffUntil > now) throw new LyricsRateLimitException(_backoffUntil - now);
                if (_nextRequest > now) await Task.Delay(_nextRequest - now, token).ConfigureAwait(false);
                _nextRequest = _clock.GetUtcNow() + _interval;
                try
                {
                    using var response = await client.GetAsync(url, HttpCompletionOption.ResponseContentRead, token).ConfigureAwait(false);
                    if (response.StatusCode == HttpStatusCode.TooManyRequests)
                    {
                        var retry = response.Headers.RetryAfter?.Delta
                            ?? (response.Headers.RetryAfter?.Date - _clock.GetUtcNow()) ?? TimeSpan.FromSeconds(15);
                        if (retry < TimeSpan.Zero) retry = TimeSpan.Zero;
                        _backoffUntil = _clock.GetUtcNow() + retry;
                        throw new LyricsRateLimitException(retry);
                    }
                    response.EnsureSuccessStatusCode();
                    await using var body = await response.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
                    return await JsonSerializer.DeserializeAsync<List<LyricsSearchResult>>(body, cancellationToken: token).ConfigureAwait(false) ?? [];
                }
                catch (HttpRequestException ex) when (attempt == 0 && ex is not LyricsRateLimitException
                    && (ex.StatusCode is null or HttpStatusCode.RequestTimeout or HttpStatusCode.InternalServerError
                        or HttpStatusCode.BadGateway or HttpStatusCode.ServiceUnavailable or HttpStatusCode.GatewayTimeout))
                { await Task.Delay(TimeSpan.FromSeconds(1), token).ConfigureAwait(false); }
                catch (TaskCanceledException) when (attempt == 0 && !token.IsCancellationRequested)
                { await Task.Delay(TimeSpan.FromSeconds(1), token).ConfigureAwait(false); }
            }
        }
        finally { _gate.Release(); }
    }
}
