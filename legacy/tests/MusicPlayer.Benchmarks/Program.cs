using System.Diagnostics;
using System.Text.Json;
using Microsoft.Data.Sqlite;
using MusicPlayer.Core;

if (args.FirstOrDefault() == "--startup-fixture")
{
    if (Environment.GetEnvironmentVariable("GITHUB_ACTIONS") != "true")
        throw new InvalidOperationException("Startup fixtures require a disposable Windows runner.");
    var destination = Path.GetFullPath(args[1]);
    var count = int.Parse(args[2]);
    if (count is not (100 or 100000)) throw new ArgumentOutOfRangeException(nameof(count));
    Directory.CreateDirectory(destination);
    var media = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyMusic), "MusicPlayerPerformance");
    Directory.CreateDirectory(media);
    var store = new LibraryStore(Path.Combine(destination, "library.db"));
    var now = DateTime.UtcNow;
    var paths = new List<string>(count);
    for (var offset = 0; offset < count; offset += 512)
    {
        var tracks = new List<Track>();
        for (var i = offset; i < Math.Min(count, offset + 512); i++)
        {
            var file = Path.Combine(media, $"track-{i:D6}.wav");
            if (!File.Exists(file))
            {
                using var writer = new BinaryWriter(File.Create(file));
                writer.Write(System.Text.Encoding.ASCII.GetBytes("RIFF")); writer.Write(918);
                writer.Write(System.Text.Encoding.ASCII.GetBytes("WAVEfmt ")); writer.Write(16);
                writer.Write((short)1); writer.Write((short)1); writer.Write(44100); writer.Write(88200);
                writer.Write((short)2); writer.Write((short)16);
                writer.Write(System.Text.Encoding.ASCII.GetBytes("data")); writer.Write(882);
                var data = new byte[882]; BitConverter.GetBytes(i).CopyTo(data, 0); writer.Write(data);
            }
            paths.Add(file);
            tracks.Add(new Track(file, $"Track {i:D6}", "Artist", "Album", "Artist", "Genre", 2026, 1,
                TimeSpan.FromMilliseconds(10), 926, now, now));
        }
        store.UpsertTracks(tracks);
    }
    store.SetSetting("last-view-context", "{\"view\":\"Songs\",\"search\":\"\"}");
    store.SetSetting("hide-exact-duplicates", "false");
    store.SetSetting("check-updates", "false");
    store.SetSetting("volume", "0");
    var session = new PlaybackSession(null, 0, paths, false, "Off", QueueIndex: -1);
    store.SaveSession(session);
    // One v8 database is restored before each revision, exercising the candidate
    // migration without giving its startup measurement a pre-migrated profile.
    using (var connection = new SqliteConnection($"Data Source={Path.Combine(destination, "library.db")}"))
    {
        connection.Open();
        using var command = connection.CreateCommand();
        command.CommandText = "UPDATE playback_session SET payload=$payload WHERE id=1; DROP TABLE playback_queue; PRAGMA user_version=8; PRAGMA wal_checkpoint(TRUNCATE);";
        command.Parameters.AddWithValue("$payload", JsonSerializer.Serialize(session));
        command.ExecuteNonQuery();
    }
    SqliteConnection.ClearAllPools();
    Console.WriteLine($"Prepared {count:N0} actual tracks and a v8 saved queue.");
    return;
}

var root = Path.GetFullPath(args.FirstOrDefault() ?? Path.Combine(Path.GetTempPath(), "music-player-benchmark-" + Guid.NewGuid().ToString("N")));
Directory.CreateDirectory(root);
var results = new List<object>();
foreach (var count in new[] { 100, 100000 })
{
    var path = Path.Combine(root, $"library-{count}.db");
    if (File.Exists(path)) throw new IOException("Use an empty benchmark output directory.");
    var store = new LibraryStore(path, new LocalAppLog(Path.Combine(root, "Logs")));
    var now = DateTime.UtcNow;
    var queue = Enumerable.Range(0, count).Select(i => Path.Combine(root, "media", $"track-{i}.wav")).ToArray();
    for (var offset = 0; offset < count; offset += 512)
        store.UpsertTracks(queue.Skip(offset).Take(512).Select((file, index) => new Track(file, $"Track {offset + index:D6}", "Artist", "Album", "Artist", "Genre", 2026, 1, TimeSpan.FromSeconds(5), 1000, now, now)));
    var session = new PlaybackSession(queue[0], 1000, queue, false, "Off", QueueIndex: 0);
    store.SaveSession(session);
    using var connection = new SqliteConnection(new SqliteConnectionStringBuilder { DataSource = path, Mode = SqliteOpenMode.ReadWriteCreate, DefaultTimeout = 30, ForeignKeys = true }.ToString());
    connection.Open();
    void Execute(string sql)
    {
        using var command = connection.CreateCommand(); command.CommandText = sql; command.ExecuteNonQuery();
    }
    Execute("PRAGMA wal_checkpoint(TRUNCATE)");
    var timer = Stopwatch.StartNew();
    // Exact pre-v9 algorithm: serialize and upsert the full session JSON.
    using (var legacy = connection.CreateCommand())
    {
        legacy.CommandText = "INSERT INTO playback_session(id,payload) VALUES(1,$payload) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload";
        legacy.Parameters.AddWithValue("$payload", JsonSerializer.Serialize(session));
        legacy.ExecuteNonQuery();
    }
    var legacyMs = timer.Elapsed.TotalMilliseconds;
    var legacyWalBytes = new FileInfo(path + "-wal").Length;
    store.SaveSession(session);
    Execute("PRAGMA wal_checkpoint(TRUNCATE)");
    timer.Restart();
    store.SaveSessionState(session with { PositionMilliseconds = 2000, Queue = [] });
    var stateMs = timer.Elapsed.TotalMilliseconds;
    var stateWalBytes = new FileInfo(path + "-wal").Length;
    timer.Restart();
    var rows = store.GetTracksPage(pageSize: 100);
    var pageMs = timer.Elapsed.TotalMilliseconds;
    if (rows.Count != Math.Min(count, 100)) throw new InvalidDataException("Fixture paging failed.");
    await store.CheckpointWalAsync();
    results.Add(new { tracks = count, legacyCheckpointMs = legacyMs, stateCheckpointMs = stateMs, legacyWalBytes, stateWalBytes, firstPageMs = pageMs });
}
var json = JsonSerializer.Serialize(results, new JsonSerializerOptions { WriteIndented = true });
File.WriteAllText(Path.Combine(root, "results.json"), json);
Console.WriteLine(json);
