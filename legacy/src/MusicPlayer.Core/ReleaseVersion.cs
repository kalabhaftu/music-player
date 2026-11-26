using System.Globalization;

namespace MusicPlayer.Core;

public readonly record struct ReleaseVersion(int Major, int Minor, int Patch, int? PreviewNumber) : IComparable<ReleaseVersion>
{
    private const string PreviewMarker = "-preview.";

    public bool IsPrerelease => PreviewNumber.HasValue;

    public int CompareTo(ReleaseVersion other)
    {
        var comparison = Major.CompareTo(other.Major);
        if (comparison == 0) comparison = Minor.CompareTo(other.Minor);
        if (comparison == 0) comparison = Patch.CompareTo(other.Patch);
        if (comparison != 0) return comparison;
        if (PreviewNumber is null) return other.PreviewNumber is null ? 0 : 1;
        if (other.PreviewNumber is null) return -1;
        return PreviewNumber.Value.CompareTo(other.PreviewNumber.Value);
    }

    public static bool TryParse(string? value, out ReleaseVersion version)
    {
        version = default;
        if (string.IsNullOrWhiteSpace(value)) return false;
        value = value.Trim();
        if (value.StartsWith('v') || value.StartsWith('V')) value = value[1..];
        var metadata = value.IndexOf('+');
        if (metadata >= 0) value = value[..metadata];

        int? preview = null;
        var previewIndex = value.IndexOf(PreviewMarker, StringComparison.OrdinalIgnoreCase);
        if (previewIndex >= 0)
        {
            if (value.IndexOf('-', previewIndex + 1) >= 0 ||
                !int.TryParse(value[(previewIndex + PreviewMarker.Length)..], NumberStyles.None, CultureInfo.InvariantCulture, out var previewNumber)) return false;
            preview = previewNumber;
            value = value[..previewIndex];
        }
        else if (value.Contains('-')) return false;

        var parts = value.Split('.');
        if (parts.Length != 3 ||
            !int.TryParse(parts[0], NumberStyles.None, CultureInfo.InvariantCulture, out var major) ||
            !int.TryParse(parts[1], NumberStyles.None, CultureInfo.InvariantCulture, out var minor) ||
            !int.TryParse(parts[2], NumberStyles.None, CultureInfo.InvariantCulture, out var patch) ||
            major < 0 || minor < 0 || patch < 0) return false;
        version = new(major, minor, patch, preview);
        return true;
    }
}
