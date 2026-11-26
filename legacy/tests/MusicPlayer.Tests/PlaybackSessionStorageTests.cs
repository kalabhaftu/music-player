using System.Text.Json;
using Microsoft.Data.Sqlite;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class PlaybackSessionStorageTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-session-" + Guid.NewGuid().ToString("N"));
    private string Database => Path.Combine(_root, "library.db");
    private LibraryStore Store() => new(Database, new LocalAppLog(Path.Combine(_root, "Logs")));

    [Fact]
    public void V8_migration_preserves_duplicate_missing_paths_and_original_backup()
    {
        var old = new PlaybackSession("missing.flac", 12345, ["missing.flac", "other.wav", "missing.flac"], true, "Queue", 2, 8, 2);
        Store();
        using (var connection = Open())
        {
            Execute(connection, "DROP TABLE playback_queue; PRAGMA user_version=8;");
            using var insert = connection.CreateCommand();
            insert.CommandText = "INSERT INTO playback_session VALUES(1,$payload)";
            insert.Parameters.AddWithValue("$payload", JsonSerializer.Serialize(old));
            insert.ExecuteNonQuery();
        }
        var migrated = Store().LoadSession()!;
        Assert.Equal(old.Queue, migrated.Queue);
        Assert.Equal(old with { Queue = migrated.Queue }, migrated);
        var backup = Assert.Single(Directory.GetFiles(_root, "*.migration-v9-*.bak"));
        using var saved = new SqliteConnection($"Data Source={backup}");
        saved.Open();
        Assert.Equal(8L, Scalar(saved, "PRAGMA user_version"));
        Assert.Equal(JsonSerializer.Serialize(old), Scalar(saved, "SELECT payload FROM playback_session"));
    }

    [Theory]
    [InlineData(3)]
    [InlineData(100000)]
    public void Checkpoint_never_touches_queue_and_payload_size_is_independent_of_queue_length(int count)
    {
        var store = Store();
        var queue = Enumerable.Range(0, count).Select(i => $"unavailable-{i % 10}.flac").ToArray();
        var session = new PlaybackSession(queue[0], 123, queue, false, "Off", QueueIndex: 0);
        store.SaveSession(session);
        using var connection = Open();
        Execute(connection, """
            CREATE TRIGGER protect_queue_delete BEFORE DELETE ON playback_queue BEGIN SELECT RAISE(ABORT,'checkpoint rewrote queue'); END;
            CREATE TRIGGER protect_queue_insert BEFORE INSERT ON playback_queue BEGIN SELECT RAISE(ABORT,'checkpoint rewrote queue'); END;
            CREATE TRIGGER protect_queue_update BEFORE UPDATE ON playback_queue BEGIN SELECT RAISE(ABORT,'checkpoint rewrote queue'); END;
            """);
        Assert.True(store.SaveSessionState(session with { PositionMilliseconds = 999, Queue = [] }));
        Assert.InRange((long)Scalar(connection, "SELECT length(payload) FROM playback_session")!, 1, 350);
        var restored = Store().LoadSession()!;
        Assert.Equal(queue, restored.Queue);
        Assert.Equal(999, restored.PositionMilliseconds);
    }

    [Fact]
    public void Identical_state_and_setting_writes_leave_database_version_unchanged()
    {
        var store = Store();
        var session = new PlaybackSession(null, 0, [], false, "Off");
        Assert.False(store.SaveSessionState(session));
        store.SaveSession(session);
        store.SetSetting("theme", "Dark");
        using var observer = Open();
        var version = Scalar(observer, "PRAGMA data_version");
        Assert.True(store.SaveSessionState(session));
        store.SetSetting("theme", "Dark");
        Assert.Equal(version, Scalar(observer, "PRAGMA data_version"));
        store.SetSetting("theme", "Light");
        Assert.NotEqual(version, Scalar(observer, "PRAGMA data_version"));
    }

    [Fact]
    public void Corruption_rebuild_preserves_queue_even_without_indexed_tracks()
    {
        var store = Store();
        store.SaveSession(new("missing.wav", 55, ["missing.wav", "missing.wav"], false, "Track", QueueIndex: 1));
        Assert.True(store.RepairCorruptDatabase(force: true));
        var session = Store().LoadSession()!;
        Assert.Equal(new[] { "missing.wav", "missing.wav" }, session.Queue);
        Assert.Equal(1, session.QueueIndex);
    }

    private SqliteConnection Open()
    {
        var connection = new SqliteConnection($"Data Source={Database}");
        connection.Open();
        return connection;
    }
    private static void Execute(SqliteConnection connection, string sql)
    {
        using var command = connection.CreateCommand();
        command.CommandText = sql;
        command.ExecuteNonQuery();
    }
    private static object? Scalar(SqliteConnection connection, string sql)
    {
        using var command = connection.CreateCommand();
        command.CommandText = sql;
        return command.ExecuteScalar();
    }
    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, true);
    }
}
