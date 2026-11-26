using System.Text.Json;
using Microsoft.Data.Sqlite;

namespace MusicPlayer.Core;

public sealed partial class LibraryStore
{
    /// <summary>Updates playback metadata without enumerating or rewriting the saved queue.
    /// Returns false if a full session has not been saved yet.</summary>
    public bool SaveSessionState(PlaybackSession session)
    {
        lock (_sessionGate)
        {
            session = SanitizeSession(session, includeQueue: false);
            using var connection = Open();
            using var transaction = connection.BeginTransaction();
            using var exists = connection.CreateCommand();
            exists.Transaction = transaction;
            exists.CommandText = "SELECT 1 FROM playback_session WHERE id=1";
            if (exists.ExecuteScalar() is null) return false;
            WriteSessionState(connection, transaction, session, create: false);
            transaction.Commit();
            return true;
        }
    }


    private static void WriteSessionState(SqliteConnection connection, SqliteTransaction transaction,
        PlaybackSession session, bool create)
    {
        using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = create
            ? "INSERT INTO playback_session(id,payload) VALUES(1,$payload) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload WHERE playback_session.payload<>excluded.payload"
            : "UPDATE playback_session SET payload=$payload WHERE id=1 AND payload<>$payload";
        Add(command, "$payload", JsonSerializer.Serialize(session with { Queue = Array.Empty<string>() }));
        command.ExecuteNonQuery();
    }

    private static void ReplaceSessionQueue(SqliteConnection connection, SqliteTransaction transaction,
        IReadOnlyList<string> queue)
    {
        using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "DELETE FROM playback_queue";
        command.ExecuteNonQuery();
        command.CommandText = "INSERT INTO playback_queue(position,track_path) VALUES($position,$path)";
        var position = command.Parameters.Add("$position", SqliteType.Integer);
        var path = command.Parameters.Add("$path", SqliteType.Text);
        for (var index = 0; index < queue.Count; index++)
        {
            position.Value = index;
            path.Value = queue[index];
            command.ExecuteNonQuery();
        }
    }

    private void NormalizeLegacySession(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "SELECT payload FROM playback_session WHERE id=1";
        if (command.ExecuteScalar() is not string payload) return;
        try
        {
            var session = JsonSerializer.Deserialize<PlaybackSession>(payload);
            if (session?.Queue is not { Count: > 0 }) return;
            ReplaceSessionQueue(connection, transaction, session.Queue);
            WriteSessionState(connection, transaction, session, create: false);
        }
        catch (JsonException ex)
        {
            // Preserve the original payload and migration backup for diagnosis/recovery.
            _log.Warning("session", "Could not migrate invalid playback session JSON; the original payload was preserved.", ex);
        }
    }

    /// <summary>Copies available WAL pages without waiting for readers or writers.</summary>
    public async Task CheckpointWalAsync()
    {
        await Task.Run(() =>
        {
            try
            {
                using var connection = Open();
                using var command = connection.CreateCommand();
                command.CommandText = "PRAGMA busy_timeout=0; PRAGMA wal_checkpoint(PASSIVE)";
                command.ExecuteNonQuery();
            }
            catch (SqliteException ex)
            {
                _log.Warning("database", "Deferred background WAL maintenance; committed data remains durable.", ex);
            }
        }).ConfigureAwait(false);
    }
}
