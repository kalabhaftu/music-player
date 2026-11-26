using System.Net;
using System.Net.Http.Headers;
using System.Text;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LrclibSearchClientTests
{
    [Fact]
    public void DownloadedLrcWithBomAndTrailingOffsetRetainsTimingOnSaveAndReload()
    {
        var document = Lyrics.Parse("\uFEFF[ar:Artist]\n[00:01.00][00:02.00]Line\n[offset:-1500]");
        Assert.Equal(TimeSpan.FromMilliseconds(-1500), document.Offset);
        Assert.Equal("Artist", document.Metadata!["ar"]);
        var reparsed = Lyrics.Parse(Lyrics.Format(document));
        Assert.Equal(document.Offset, reparsed.Offset);
        Assert.Equal(document.Lines, reparsed.Lines);
        Assert.Equal("Line", reparsed.At(TimeSpan.Zero));
    }

    [Fact]
    public async Task ConnectionFailureAllowsImmediateUserRetry()
    {
        var calls = 0;
        using var http = new HttpClient(new Handler(_ => ++calls <= 2 ? throw new HttpRequestException("Offline") : Json()));
        var client = new LrclibSearchClient(http, requestInterval: TimeSpan.Zero);
        await Assert.ThrowsAsync<HttpRequestException>(() => client.SearchAsync("Song", "Artist"));
        Assert.Single(await client.SearchAsync("Song", "Artist"));
        Assert.Equal(3, calls);
    }

    [Fact]
    public async Task TransientServerErrorRetriesOnceAndKeepsTimestampsAndDuration()
    {
        var calls = 0;
        using var http = new HttpClient(new Handler(_ => ++calls == 1 ? new(HttpStatusCode.ServiceUnavailable) : Json()));
        var result = Assert.Single(await new LrclibSearchClient(http, requestInterval: TimeSpan.Zero).SearchAsync("Song", "Artist"));
        Assert.Equal("[00:01.00]Line", result.SyncedLyrics);
        Assert.Equal(180, result.Duration);
        Assert.Equal(2, calls);
    }

    [Fact]
    public async Task RealRateLimitHonorsServerRetryAfterWithoutSendingAnotherRequest()
    {
        var calls = 0;
        var clock = new Clock();
        using var http = new HttpClient(new Handler(_ =>
        {
            if (++calls > 1) return Json();
            var response = new HttpResponseMessage(HttpStatusCode.TooManyRequests);
            response.Headers.RetryAfter = new RetryConditionHeaderValue(TimeSpan.FromSeconds(75));
            return response;
        }));
        var client = new LrclibSearchClient(http, clock, TimeSpan.Zero);
        var error = await Assert.ThrowsAsync<LyricsRateLimitException>(() => client.SearchAsync("Song", "Artist"));
        Assert.Equal(TimeSpan.FromSeconds(75), error.RetryAfter);
        clock.Now += TimeSpan.FromSeconds(15);
        error = await Assert.ThrowsAsync<LyricsRateLimitException>(() => client.SearchAsync("Other", "Artist"));
        Assert.Equal(TimeSpan.FromSeconds(60), error.RetryAfter);
        Assert.Equal(1, calls);
        clock.Now += TimeSpan.FromSeconds(61);
        Assert.Single(await client.SearchAsync("Song", "Artist"));
    }

    [Fact]
    public async Task CancellationDoesNotPoisonTheNextRequest()
    {
        using var http = new HttpClient(new Handler(_ => Json()));
        var client = new LrclibSearchClient(http, requestInterval: TimeSpan.Zero);
        using var canceled = new CancellationTokenSource(); canceled.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.SearchAsync("Song", "Artist", canceled.Token));
        Assert.Single(await client.SearchAsync("Song", "Artist"));
    }

    [Fact]
    public async Task RequestsEncodeUnicodeAndQuerySeparators()
    {
        Uri? uri = null;
        using var http = new HttpClient(new Handler(request => { uri = request.RequestUri; return Json(); }));
        await new LrclibSearchClient(http, requestInterval: TimeSpan.Zero).SearchAsync("ሙዚቃ & love", "A/B");
        Assert.Contains("%26", uri!.AbsoluteUri);
        Assert.Contains("artist_name=A%2FB", uri.AbsoluteUri);
    }

    private static HttpResponseMessage Json() => new(HttpStatusCode.OK)
    {
        Content = new StringContent("""[{"trackName":"Song","artistName":"Artist","syncedLyrics":"[00:01.00]Line","plainLyrics":"Line","duration":180}]""", Encoding.UTF8, "application/json")
    };
    private sealed class Handler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(respond(request));
    }
    private sealed class Clock : TimeProvider
    {
        public DateTimeOffset Now { get; set; } = new(2026, 10, 10, 0, 0, 0, TimeSpan.Zero);
        public override DateTimeOffset GetUtcNow() => Now;
    }
}
