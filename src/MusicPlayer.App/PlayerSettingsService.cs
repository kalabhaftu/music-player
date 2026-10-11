using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>
/// Projects and persists the settings exposed by the local player UI without depending on WinUI.
/// </summary>
internal sealed class PlayerSettingsService(LibraryStore store)
{
    private static readonly Dictionary<string, string> WebSettingKeys = new(StringComparer.Ordinal)
    {
        ["theme"] = "theme", ["accentMode"] = "accent-mode", ["accentManual"] = "accent-manual", ["accentColor"] = "accent-color",
        ["selectionColorMode"] = "selection-color-mode", ["selectionColor"] = "selection-color",
        ["transparentWindow"] = "transparent-window", ["windowTransparency"] = "window-transparency",
        ["motionStyle"] = "motion-style", ["hideDuplicates"] = "hide-exact-duplicates",
        ["autoOpenPanel"] = "auto-open-side-panel", ["autoLoadLyrics"] = "auto-load-lyrics", ["seekFromLyrics"] = "seek-from-lyrics",
        ["showArtwork"] = "show-list-artwork", ["showArtist"] = "show-list-artist",
        ["showAlbum"] = "show-list-album", ["showAdded"] = "show-list-added", ["showYear"] = "show-list-year", ["showDuration"] = "show-list-duration",
        ["showFavorite"] = "show-list-favorite", ["checkUpdates"] = "check-updates", ["minimizeToTray"] = "minimize-to-tray",
        ["navigationWidth"] = "navigation-pane-width", ["browseWidth"] = "browse-pane-width", ["navigationOrder"] = "navigation-order",
        ["hiddenPanels"] = "hidden-panels", ["rightSidebarMode"] = "right-sidebar-mode",
        ["rightPanelWidth"] = "right-panel-width", ["rightPanelOpen"] = "right-panel-open", ["sidebarCollapsed"] = "sidebar-collapsed",
        ["songColumnWidths"] = "song-column-widths"
    };

    public object ProjectWebSettings(string resolvedTheme, string? artworkAccent, object? artworkPalette, bool needsDefaultSelection = true)
    {
        var stored = store.GetSettings("");
        string Value(string key, string fallback) => stored.TryGetValue(key, out var value) ? value : fallback;
        bool On(string key, bool fallback = false) => stored.TryGetValue(key, out var value) ? value == "true" : fallback;
        bool? PanelOpen() => stored.TryGetValue("right-panel-open", out var value) ? value switch { "true" => true, "false" => false, _ => null } : null;
        bool? Collapsed() => stored.TryGetValue("sidebar-collapsed", out var value) ? value switch { "true" => true, "false" => false, _ => null } : null;
        return new
        {
            theme = Value("theme", "System"), accentMode = Value("accent-mode", "Artwork"), accentManual = On("accent-manual"),
            resolvedTheme, accentColor = Value("accent-color", "#b7ff2d"), artworkAccent, artworkPalette,
            selectionColorMode = Value("selection-color-mode", "Theme"), selectionColor = Value("selection-color", "#b7ff2d"),
            transparentWindow = On("transparent-window"), windowTransparency = Value("window-transparency", "0"),
            motionStyle = Value("motion-style", "Subtle"),
            hideDuplicates = Value("hide-exact-duplicates", "true") != "false", autoOpenPanel = Value("auto-open-side-panel", "true") != "false",
            autoLoadLyrics = On("auto-load-lyrics"), seekFromLyrics = On("seek-from-lyrics", true),
            showArtwork = Value("show-list-artwork", "true") != "false", showArtist = Value("show-list-artist", "true") != "false",
            showAlbum = Value("show-list-album", "true") != "false", showAdded = Value("show-list-added", "true") != "false",
            showYear = On("show-list-year"), showDuration = Value("show-list-duration", "true") != "false",
            showFavorite = Value("show-list-favorite", "true") != "false", checkUpdates = Value("check-updates", "true") != "false",
            minimizeToTray = On("minimize-to-tray"), navigationWidth = Value("navigation-pane-width", "232"), browseWidth = Value("browse-pane-width", "280"),
            navigationOrder = ReadStringArray("navigation-order")
                .Where(item => item is not ("Most Played" or "Recently Played"))
                .ToArray(),
            hiddenPanels = ReadStringArray("hidden-panels"),
            rightSidebarMode = Value("right-sidebar-mode", "Queue"),
            rightPanelOpen = PanelOpen(),
            rightPanelWidth = Value("right-panel-width", "294"), sidebarCollapsed = Collapsed(),
            songColumnWidths = ReadSongColumnWidths(),
            indexedLibraryBytes = store.GetLibraryStats().TotalBytes,
            needsDefaultSelection
        };
    }

    /// <returns>Whether an accent setting was included, so the shell can refresh its visual accent.</returns>
    public bool PersistWebSettings(JsonElement payload)
    {
        if (!payload.TryGetProperty("settings", out var settings) || settings.ValueKind != JsonValueKind.Object)
            throw new ArgumentException("Settings are missing.");

        foreach (var item in settings.EnumerateObject())
        {
            if (!WebSettingKeys.TryGetValue(item.Name, out var key))
                throw new ArgumentException($"The setting '{item.Name}' is not editable from the player.");
            var value = item.Value.ValueKind switch
            {
                JsonValueKind.True => "true", JsonValueKind.False => "false", JsonValueKind.String => item.Value.GetString() ?? "",
                JsonValueKind.Array => item.Value.GetRawText(), JsonValueKind.Number => item.Value.ToString(),
                JsonValueKind.Object => item.Value.GetRawText(),
                _ => throw new ArgumentException($"The value for '{item.Name}' is invalid.")
            };
            ValidateSetting(item.Name, value);
            store.SetSetting(key, value);
        }

        return settings.EnumerateObject().Any(item => item.Name is "accentMode" or "accentManual" or "accentColor");
    }

    public bool ResetUiSettings(JsonElement payload)
    {
        if (!payload.TryGetProperty("groups", out var groups) || groups.ValueKind != JsonValueKind.Array)
            throw new ArgumentException("Choose one or more interface sections to reset.");

        var selected = groups.EnumerateArray()
            .Where(item => item.ValueKind == JsonValueKind.String)
            .Select(item => item.GetString()!)
            .Distinct(StringComparer.Ordinal)
            .ToArray();
        if (selected.Length == 0 || selected.Any(group => group is not ("appearance" or "layout" or "libraryDisplay")))
            throw new ArgumentException("The selected interface reset section is not available.");

        var appearanceChanged = selected.Contains("appearance", StringComparer.Ordinal);
        if (appearanceChanged)
        {
            store.SetSetting("theme", "System");
            store.SetSetting("accent-mode", "Artwork");
            store.SetSetting("accent-manual", "false");
            store.SetSetting("accent-color", "#b7ff2d");
            store.SetSetting("selection-color-mode", "Theme");
            store.SetSetting("selection-color", "#b7ff2d");
            store.SetSetting("transparent-window", "false");
            store.SetSetting("window-transparency", "0");
            store.SetSetting("motion-style", "Subtle");
        }
        if (selected.Contains("layout", StringComparer.Ordinal))
        {
            store.SetSetting("navigation-pane-width", "232");
            store.SetSetting("right-panel-width", "294");
            store.SetSetting("sidebar-collapsed", "auto");
            store.SetSetting("browse-pane-width", "280");
            store.SetSetting("auto-open-side-panel", "true");
            store.SetSetting("right-sidebar-mode", "Queue");
            store.SetSetting("right-panel-open", "auto");
            store.SetSetting("navigation-order", "[\"Home\",\"Search\",\"Favorites\",\"Songs\",\"Albums\",\"Artists\",\"Genres\",\"Recently Added\",\"Playlists\",\"Folders\"]");
            store.SetSetting("hidden-panels", "[]");
        }
        if (selected.Contains("libraryDisplay", StringComparer.Ordinal))
        {
            store.SetSetting("hide-exact-duplicates", "true");
            store.SetSetting("show-list-artwork", "true");
            store.SetSetting("show-list-artist", "true");
            store.SetSetting("show-list-album", "true");
            store.SetSetting("show-list-added", "true");
            store.SetSetting("show-list-year", "false");
            store.SetSetting("show-list-duration", "true");
            store.SetSetting("show-list-favorite", "true");
            store.SetSetting("song-column-widths", "{}");
        }
        return appearanceChanged;
    }

    private string[] ReadStringArray(string key)
    {
        try { return store.GetSetting(key) is { } json ? JsonSerializer.Deserialize<string[]>(json) ?? Array.Empty<string>() : Array.Empty<string>(); }
        catch (JsonException ex)
        {
            LocalAppLog.Shared.Warning("settings", $"Saved setting '{key}' was invalid JSON.", ex);
            return Array.Empty<string>();
        }
    }

    private Dictionary<string, int> ReadSongColumnWidths()
    {
        try
        {
            var json = store.GetSetting("song-column-widths");
            if (string.IsNullOrWhiteSpace(json)) return new(StringComparer.Ordinal);
            using var document = JsonDocument.Parse(json);
            if (!IsValidSongColumnWidths(document.RootElement)) return new(StringComparer.Ordinal);
            return document.RootElement.EnumerateObject().ToDictionary(item => item.Name, item => item.Value.GetInt32(), StringComparer.Ordinal);
        }
        catch (JsonException ex)
        {
            LocalAppLog.Shared.Warning("settings", "Saved song column widths were invalid JSON.", ex);
            return new(StringComparer.Ordinal);
        }
    }

    private static void ValidateSetting(string key, string value)
    {
        var valid = key switch
        {
            "theme" => value is "System" or "Light" or "Dark",
            "accentMode" => value is "Native" or "Artwork",
            "selectionColorMode" => value is "Theme" or "Custom",
            "accentManual" or "transparentWindow" or "hideDuplicates" or "autoOpenPanel" or "autoLoadLyrics" or "seekFromLyrics" or "showArtwork" or "showArtist" or "showAlbum" or "showAdded" or "showYear" or "showDuration" or "showFavorite" or "checkUpdates" or "minimizeToTray" or "rightPanelOpen" => value is "true" or "false",
            "windowTransparency" => int.TryParse(value, out var transparency) && transparency is >= 0 and <= 60,
            "motionStyle" => value is "Off" or "Subtle" or "Expressive",
            "accentColor" => System.Text.RegularExpressions.Regex.IsMatch(value, "^#[0-9a-fA-F]{6}$"),
            "selectionColor" => System.Text.RegularExpressions.Regex.IsMatch(value, "^#[0-9a-fA-F]{6}$"),
            "navigationWidth" => int.TryParse(value, out var nav) && nav is >= 180 and <= 360,
            "rightPanelWidth" => int.TryParse(value, out var right) && right is >= 240 and <= 460,
            "browseWidth" => int.TryParse(value, out var browse) && browse is >= 180 and <= 480,
            "sidebarCollapsed" => value is "true" or "false" or "auto",
            "rightSidebarMode" => value is "Queue" or "Info",
            "navigationOrder" or "hiddenPanels" => JsonLooksStringArray(value),
            "songColumnWidths" => JsonLooksSongColumnWidths(value),
            _ => false
        };
        if (!valid) throw new ArgumentException($"The value for '{key}' is not valid.");
    }

    private static bool JsonLooksStringArray(string value)
    {
        try
        {
            using var document = JsonDocument.Parse(value);
            return document.RootElement.ValueKind == JsonValueKind.Array &&
                   document.RootElement.EnumerateArray().All(item => item.ValueKind == JsonValueKind.String);
        }
        catch (JsonException) { return false; }
    }

    private static bool JsonLooksSongColumnWidths(string value)
    {
        try
        {
            using var document = JsonDocument.Parse(value);
            return IsValidSongColumnWidths(document.RootElement);
        }
        catch (JsonException) { return false; }
    }

    private static bool IsValidSongColumnWidths(JsonElement root)
    {
        if (root.ValueKind != JsonValueKind.Object) return false;
        foreach (var item in root.EnumerateObject())
        {
            var range = item.Name switch
            {
                "title" => (160, 1200), "album" => (90, 800), "added" => (76, 300),
                "year" => (50, 200), "plays" => (50, 200), "duration" => (50, 200), _ => (0, 0)
            };
            if (range.Item1 == 0 || item.Value.ValueKind != JsonValueKind.Number ||
                !item.Value.TryGetInt32(out var width) || width < range.Item1 || width > range.Item2) return false;
        }
        return true;
    }
}
