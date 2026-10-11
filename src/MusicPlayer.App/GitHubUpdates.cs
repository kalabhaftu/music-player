using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text.Json.Serialization;
using MusicPlayer.Core;

namespace MusicPlayer.App;

internal sealed record GitHubRelease(
    [property: JsonPropertyName("tag_name")] string Tag,
    [property: JsonPropertyName("html_url")] string Url,
    [property: JsonPropertyName("draft")] bool Draft,
    [property: JsonPropertyName("prerelease")] bool Prerelease);

internal sealed record GitHubUpdateCheck(GitHubRelease? Release, string? ETag);

/// <summary>Checks GitHub release metadata. Installing an update remains a deliberate user action.</summary>
internal static class GitHubUpdates
{
    private const string Repository = "kalabhaftu/music-player";
    public static string RepositoryUrl => $"https://github.com/{Repository}";
    public static string ReleasesUrl => $"{RepositoryUrl}/releases";
    private static readonly Uri ReleasesApi = new($"https://api.github.com/repos/{Repository}/releases?per_page=10");
    private static readonly Uri LatestStablePage = new($"https://github.com/{Repository}/releases/latest");
    private static readonly HttpClient Client = CreateClient();

    private static HttpClient CreateClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromSeconds(8) };
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("MusicPlayer", typeof(GitHubUpdates).Assembly.GetName().Version?.ToString(3) ?? "0.1.0"));
        client.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("application/vnd.github+json"));
        client.DefaultRequestHeaders.Add("X-GitHub-Api-Version", "2022-11-28");
        return client;
    }

    public static GitHubRelease? ReadCachedRelease(string? tag, string? url)
    {
        if (!ReleaseVersion.TryParse(tag, out _) || !IsReleasePage(url)) return null;
        return new(tag!, url!, Draft: false, Prerelease: tag!.Contains("-preview.", StringComparison.OrdinalIgnoreCase));
    }

    public static async Task<GitHubUpdateCheck> GetLatestAsync(
        bool includePrerelease,
        string? etag,
        GitHubRelease? cachedRelease,
        CancellationToken cancellationToken = default)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(TimeSpan.FromSeconds(8));
        cancellationToken = timeout.Token;
        using var request = new HttpRequestMessage(HttpMethod.Get, ReleasesApi);
        if (IsEligible(cachedRelease, includePrerelease) && EntityTagHeaderValue.TryParse(etag, out var parsedEtag))
            request.Headers.IfNoneMatch.Add(parsedEtag);

        using var response = await Client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotModified)
            return new(IsEligible(cachedRelease, includePrerelease) ? cachedRelease : null, response.Headers.ETag?.ToString() ?? etag);

        // GitHub's API occasionally rate-limits unauthenticated clients. The stable
        // releases/latest endpoint is a narrow fallback for stable builds only; its
        // final redirect must identify this exact repository and a tagged release.
        if ((response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.TooManyRequests) && !includePrerelease)
            return new(await GetStableReleaseFromRedirectAsync(cancellationToken).ConfigureAwait(false), null);

        response.EnsureSuccessStatusCode();
        var releases = await response.Content.ReadFromJsonAsync<GitHubRelease[]>(cancellationToken: cancellationToken).ConfigureAwait(false);
        if (releases is null) throw new InvalidDataException("GitHub returned an empty release listing.");

        var latest = releases
            .Where(release => IsEligible(release, includePrerelease))
            .Select(release => (Release: release, Valid: ReleaseVersion.TryParse(release.Tag, out var version), Version: version))
            .Where(item => item.Valid)
            .OrderByDescending(item => item.Version)
            .Select(item => item.Release)
            .FirstOrDefault();
        return new(latest, response.Headers.ETag?.ToString());
    }

    public static async Task<IReadOnlyList<GitHubRelease>> GetReleaseHistoryAsync(CancellationToken cancellationToken = default)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(TimeSpan.FromSeconds(8));
        cancellationToken = timeout.Token;
        using var response = await Client.GetAsync(ReleasesApi, HttpCompletionOption.ResponseHeadersRead, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var releases = await response.Content.ReadFromJsonAsync<GitHubRelease[]>(cancellationToken: cancellationToken).ConfigureAwait(false);
        return (releases ?? [])
            .Where(release => !release.Draft && ReleaseVersion.TryParse(release.Tag, out _) && IsReleasePage(release.Url))
            .OrderByDescending(release => ReleaseVersion.TryParse(release.Tag, out var version) ? version : default)
            .Take(10)
            .ToArray();
    }

    public static bool IsReleasePage(string? value)
    {
        if (!Uri.TryCreate(value, UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps ||
            !uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase) || !string.IsNullOrEmpty(uri.UserInfo)) return false;
        return uri.AbsolutePath.StartsWith($"/{Repository}/releases/tag/", StringComparison.OrdinalIgnoreCase);
    }

    public static bool IsRepositoryInfoPage(string? value)
    {
        if (!Uri.TryCreate(value, UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps ||
            !uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase) || !string.IsNullOrEmpty(uri.UserInfo)) return false;
        var path = uri.AbsolutePath.TrimEnd('/');
        return path.Equals($"/{Repository}", StringComparison.OrdinalIgnoreCase) ||
            path.Equals($"/{Repository}/releases", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsEligible(GitHubRelease? release, bool includePrerelease) =>
        release is not null && !release.Draft && (includePrerelease || !release.Prerelease) &&
        ReleaseVersion.TryParse(release.Tag, out _) && IsReleasePage(release.Url);

    private static async Task<GitHubRelease?> GetStableReleaseFromRedirectAsync(CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, LatestStablePage);
        using var response = await Client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var finalUri = response.RequestMessage?.RequestUri;
        if (finalUri is null || !finalUri.Scheme.Equals(Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase) ||
            !finalUri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase))
            throw new HttpRequestException("GitHub's stable release redirect left the trusted GitHub host.");

        const string tagPrefix = $"/{Repository}/releases/tag/";
        if (!finalUri.AbsolutePath.StartsWith(tagPrefix, StringComparison.OrdinalIgnoreCase)) return null;
        var tag = Uri.UnescapeDataString(finalUri.AbsolutePath[tagPrefix.Length..]);
        if (!ReleaseVersion.TryParse(tag, out _) || tag.Contains("-preview.", StringComparison.OrdinalIgnoreCase)) return null;
        var url = new Uri(finalUri.GetLeftPart(UriPartial.Path)).AbsoluteUri;
        return IsReleasePage(url) ? new(tag, url, Draft: false, Prerelease: false) : null;
    }
}
