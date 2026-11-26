using System.Buffers;
using System.Globalization;
using System.Security;
using System.Security.Cryptography;
using Microsoft.Data.Sqlite;

namespace MusicPlayer.Core;

/// <summary>A byte-identical file group in the current query, with a stable representative.</summary>
public sealed record ExactDuplicateGroup(string Sha256, long FileSize, int CopyCount, Track Representative);
public sealed record ExactDuplicateGroupPage(IReadOnlyList<ExactDuplicateGroup> Groups, int TotalCount);
public sealed record ExactDuplicateCopiesPage(IReadOnlyList<Track> Tracks, int TotalCount);
public sealed record TrackPageResult(IReadOnlyList<Track> Tracks, int TotalCount);

public sealed partial class LibraryStore
{
    private readonly object _fingerprintGate = new();
    private volatile bool _fingerprintSnapshotCurrent;
    private readonly object _fingerprintRefreshGate = new();
    private long _fingerprintGeneration;
    private void InvalidateFingerprintState() { Interlocked.Increment(ref _fingerprintGeneration); _fingerprintSnapshotCurrent = false; }
    private DateTime _fingerprintSnapshotValidatedUtc;
    private DateTime _unavailableFingerprintRetryUtc;
    private List<string> _unavailableFingerprintPaths = [];
    private static readonly TimeSpan FingerprintSnapshotMaxAge = TimeSpan.FromMinutes(30);
    private static readonly TimeSpan UnavailableFingerprintRetryInterval = TimeSpan.FromMinutes(1);

    private sealed record FingerprintCandidate(string Path, long? CachedSize, string? CachedModified, string? CachedSha256);
    private sealed record FingerprintUpdate(string Path, long? Size, string? Modified, string? Sha256);
    private sealed record FingerprintWorkItem(string Path, bool Available, long? Size, string? Modified,
        long? CachedSize, string? CachedModified, string? CachedSha256);

    private const int FingerprintStateBatchSize = 512;
    private const int FingerprintHashBatchSize = 32;

    /// <summary>Reads one paged track result and its total with only one duplicate-index refresh.</summary>
    public TrackPageResult GetTracksPageWithCount(
        string? search = null, TrackSort sort = TrackSort.Title, bool descending = false,
        string? filter = null, string? groupColumn = null, string? groupValue = null,
        int offset = 0, int pageSize = 200, bool hideExactDuplicates = false, CancellationToken cancellationToken = default)
    {
        ValidatePage(offset, pageSize, "tracks");
        if (hideExactDuplicates && !UseCachedFingerprints) PrepareExactFingerprints(cancellationToken);
        var page = QueryTracks(search, sort, descending, filter, groupColumn, groupValue, offset, pageSize,
            countOnly: false, hideExactDuplicates, cancellationToken, fingerprintsAlreadyCurrent: hideExactDuplicates);
        var total = QueryTracks(search, sort, descending, filter, groupColumn, groupValue, null, 0,
            countOnly: true, hideExactDuplicates, cancellationToken, fingerprintsAlreadyCurrent: hideExactDuplicates).Count;
        return new(page.Tracks, total);
    }

    /// <summary>
    /// Returns one representative per exact-content duplicate group where at least one copy
    /// matches the supplied search, filter, and group criteria. CopyCount includes all copies.
    /// </summary>
    public IReadOnlyList<ExactDuplicateGroup> GetDuplicateGroups(
        string? search = null, TrackSort sort = TrackSort.Title, bool descending = false,
        string? filter = null, string? groupColumn = null, string? groupValue = null,
        int offset = 0, int pageSize = 200, CancellationToken cancellationToken = default)
    {
        ValidatePage(offset, pageSize, "duplicate groups");
        if (!UseCachedFingerprints) PrepareExactFingerprints(cancellationToken);
        var query = BuildTrackQuery(search, sort, descending, filter, groupColumn, groupValue);
        return QueryDuplicateGroups(query, offset, pageSize, cancellationToken);
    }

    /// <summary>Returns a duplicate-group page and its total after a single fingerprint refresh.</summary>
    public ExactDuplicateGroupPage GetDuplicateGroupsPage(
        string? search = null, TrackSort sort = TrackSort.Title, bool descending = false,
        string? filter = null, string? groupColumn = null, string? groupValue = null,
        int offset = 0, int pageSize = 200, CancellationToken cancellationToken = default)
    {
        ValidatePage(offset, pageSize, "duplicate groups");
        if (!UseCachedFingerprints) PrepareExactFingerprints(cancellationToken);
        var query = BuildTrackQuery(search, sort, descending, filter, groupColumn, groupValue);
        var groups = QueryDuplicateGroups(query, offset, pageSize, cancellationToken);
        var total = CountDuplicateGroups(query, cancellationToken);
        return new(groups, total);
    }

    private IReadOnlyList<ExactDuplicateGroup> QueryDuplicateGroups(TrackQuery query, int offset, int pageSize, CancellationToken cancellationToken)
    {
        var duplicateSetQuery = UseCachedFingerprints
            ? "SELECT f.sha256,f.file_size,COUNT(*) AS copy_count FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.file_size=t.file_size AND f.modified_utc=t.modified_utc AND musicplayer_visible(t.path) GROUP BY f.sha256,f.file_size HAVING COUNT(*)>1"
            : "SELECT sha256,file_size,COUNT(*) AS copy_count FROM track_fingerprints GROUP BY sha256,file_size HAVING COUNT(*)>1";
        using var connection = Open(); using var command = connection.CreateCommand();
        command.CommandText = $"""
            {BuildFingerprintCte(query)}, duplicate_sets AS ({duplicateSetQuery}), matching_duplicates AS (
              SELECT f.*,d.file_size AS duplicate_file_size,d.copy_count
              FROM filtered_tracks f JOIN duplicate_sets d
                ON d.sha256=f.exact_fingerprint AND d.file_size=f.fingerprint_size
            ), ranked_duplicates AS (
              SELECT matching_duplicates.*,
                ROW_NUMBER() OVER (PARTITION BY exact_fingerprint,fingerprint_size ORDER BY {query.OrderBy}) AS group_rank
              FROM matching_duplicates
            )
            SELECT * FROM ranked_duplicates WHERE group_rank=1 ORDER BY {query.OrderBy} LIMIT $limit OFFSET $offset
            """;
        BindTrackQuery(command, query); Add(command, "$limit", pageSize); Add(command, "$offset", offset);
        using var reader = command.ExecuteReader();
        var groups = new List<ExactDuplicateGroup>(pageSize);
        while (reader.Read())
        {
            cancellationToken.ThrowIfCancellationRequested();
            groups.Add(new(reader.GetString(reader.GetOrdinal("exact_fingerprint")),
                reader.GetInt64(reader.GetOrdinal("duplicate_file_size")),
                reader.GetInt32(reader.GetOrdinal("copy_count")), ReadTrack(reader)));
        }
        return groups;
    }

    /// <summary>Counts duplicate groups using the same filters as <see cref="GetDuplicateGroups"/>.</summary>
    public int CountDuplicateGroups(string? search = null, TrackSort sort = TrackSort.Title, bool descending = false,
        string? filter = null, string? groupColumn = null, string? groupValue = null,
        CancellationToken cancellationToken = default)
    {
        if (!UseCachedFingerprints) PrepareExactFingerprints(cancellationToken);
        var query = BuildTrackQuery(search, sort, descending, filter, groupColumn, groupValue);
        return CountDuplicateGroups(query, cancellationToken);
    }

    private int CountDuplicateGroups(TrackQuery query, CancellationToken cancellationToken)
    {
        var duplicateSetQuery = UseCachedFingerprints
            ? "SELECT f.sha256,f.file_size FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.file_size=t.file_size AND f.modified_utc=t.modified_utc AND musicplayer_visible(t.path) GROUP BY f.sha256,f.file_size HAVING COUNT(*)>1"
            : "SELECT sha256,file_size FROM track_fingerprints GROUP BY sha256,file_size HAVING COUNT(*)>1";
        using var connection = Open(); using var command = connection.CreateCommand();
        command.CommandText = $"""
            {BuildFingerprintCte(query)}, duplicate_sets AS ({duplicateSetQuery})
            SELECT COUNT(*) FROM (
              SELECT f.exact_fingerprint,f.fingerprint_size
              FROM filtered_tracks f JOIN duplicate_sets d
                ON d.sha256=f.exact_fingerprint AND d.file_size=f.fingerprint_size
              GROUP BY f.exact_fingerprint,f.fingerprint_size
            )
            """;
        BindTrackQuery(command, query);
        cancellationToken.ThrowIfCancellationRequested();
        return Convert.ToInt32(command.ExecuteScalar(), CultureInfo.InvariantCulture);
    }

    /// <summary>Returns every indexed copy for an exact-content group, in stable path order.</summary>
    public IReadOnlyList<Track> GetDuplicateTracks(string sha256, int offset = 0, int pageSize = 200,
        CancellationToken cancellationToken = default)
    {
        return GetDuplicateTracksPage(sha256, offset, pageSize, cancellationToken).Tracks;
    }

    /// <summary>Returns one copy page and a current copy count, validating just that fingerprint group.</summary>
    public ExactDuplicateCopiesPage GetDuplicateTracksPage(string sha256, int offset = 0, int pageSize = 200,
        CancellationToken cancellationToken = default)
    {
        ValidatePage(offset, pageSize, "duplicate copies");
        if (sha256 is null || sha256.Length != 64 || !sha256.All(Uri.IsHexDigit))
            throw new ArgumentException("A duplicate group key must be a 64-character SHA-256 hex string.", nameof(sha256));
        sha256 = sha256.ToUpperInvariant();
        if (!UseCachedFingerprints) EnsureFingerprintGroupCurrent(sha256, cancellationToken);
        using var connection = Open();
        using var count = connection.CreateCommand();
        var currentFingerprint = UseCachedFingerprints
            ? " AND f.file_size=t.file_size AND f.modified_utc=t.modified_utc AND musicplayer_visible(t.path)"
            : string.Empty;
        count.CommandText = "SELECT COUNT(*) FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.sha256=$hash" + currentFingerprint;
        Add(count, "$hash", sha256);
        var total = Convert.ToInt32(count.ExecuteScalar(), CultureInfo.InvariantCulture);
        using var command = connection.CreateCommand();
        command.CommandText = "SELECT t.* FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.sha256=$hash" + currentFingerprint + " ORDER BY t.path COLLATE NOCASE,t.path LIMIT $limit OFFSET $offset";
        Add(command, "$hash", sha256); Add(command, "$limit", pageSize); Add(command, "$offset", offset);
        using var reader = command.ExecuteReader();
        var tracks = new List<Track>(pageSize);
        while (reader.Read()) { cancellationToken.ThrowIfCancellationRequested(); tracks.Add(ReadTrack(reader)); }
        return new(tracks, total);
    }

    private void EnsureFingerprintGroupCurrent(string sha256, CancellationToken cancellationToken)
    {
        lock (_fingerprintRefreshGate)
        {
            var snapshotInvalidated = false;
            string? afterPath = null;
            while (true)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var items = ReadFingerprintGroupBatch(sha256, afterPath);
                if (items.Count == 0) break;
                var updates = new List<FingerprintUpdate>(items.Count);
                foreach (var item in items)
                {
                    if (cancellationToken.IsCancellationRequested)
                    {
                        using var connection = Open();
                        WriteFingerprintUpdates(connection, updates);
                        if (snapshotInvalidated) InvalidateFingerprintState();
                        cancellationToken.ThrowIfCancellationRequested();
                    }
                    try
                    {
                        if (!TryGetFileState(item.Path, out var size, out var modified))
                        {
                            updates.Add(new(item.Path, null, null, null));
                            snapshotInvalidated = true;
                            continue;
                        }
                        if (item.CachedSize == size && item.CachedModified is { } cachedStamp && DateTime.Equals(ParseStamp(cachedStamp), modified))
                            continue;
                        var hash = HashFile(item.Path, cancellationToken);
                        if (!TryGetFileState(item.Path, out var finalSize, out var finalModified) || finalSize != size || finalModified != modified)
                        {
                            updates.Add(new(item.Path, null, null, null));
                            snapshotInvalidated = true;
                        }
                        else
                        {
                            updates.Add(new(item.Path, size, Stamp(modified), hash));
                            snapshotInvalidated = true;
                        }
                    }
                    catch (OperationCanceledException)
                    {
                        using var connection = Open();
                        WriteFingerprintUpdates(connection, updates);
                        if (snapshotInvalidated) InvalidateFingerprintState();
                        throw;
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException or FormatException)
                    {
                        updates.Add(new(item.Path, null, null, null));
                        snapshotInvalidated = true;
                        _log.Warning("duplicates", $"Could not verify duplicate fingerprint for '{item.Path}'.", ex);
                    }
                }
                using (var connection = Open()) WriteFingerprintUpdates(connection, updates);
                afterPath = items[^1].Path;
            }
            if (snapshotInvalidated) InvalidateFingerprintState();
        }
    }

    private List<FingerprintWorkItem> ReadFingerprintGroupBatch(string sha256, string? afterPath)
    {
        using var connection = Open(); using var command = connection.CreateCommand();
        command.CommandText = afterPath is null
            ? "SELECT t.path,f.file_size,f.modified_utc,f.file_size,f.modified_utc,f.sha256 FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.sha256=$hash ORDER BY t.path LIMIT $limit"
            : "SELECT t.path,f.file_size,f.modified_utc,f.file_size,f.modified_utc,f.sha256 FROM tracks t JOIN track_fingerprints f ON f.path=t.path WHERE f.sha256=$hash AND t.path>$after ORDER BY t.path LIMIT $limit";
        Add(command, "$hash", sha256);
        if (afterPath is not null) Add(command, "$after", afterPath);
        Add(command, "$limit", FingerprintHashBatchSize);
        using (var reader = command.ExecuteReader())
        {
            var items = new List<FingerprintWorkItem>(FingerprintHashBatchSize);
            while (reader.Read()) items.Add(new(reader.GetString(0), true, null, null,
                reader.GetInt64(3), reader.GetString(4), reader.GetString(5)));
            return items;
        }
    }

    private string BuildFingerprintCte(TrackQuery query) => $"""
        WITH filtered_tracks AS (
          SELECT tracks.*,fingerprints.sha256 AS exact_fingerprint,fingerprints.file_size AS fingerprint_size
          FROM tracks LEFT JOIN track_fingerprints fingerprints ON fingerprints.path=tracks.path {(UseCachedFingerprints ? "AND fingerprints.file_size=tracks.file_size AND fingerprints.modified_utc=tracks.modified_utc" : "")}
          WHERE {query.Where}
        )
        """;

    private string BuildDeduplicatedCte(TrackQuery query) => $"""
        {BuildFingerprintCte(query)}, ranked_tracks AS (
          SELECT filtered_tracks.*,
            ROW_NUMBER() OVER (
              PARTITION BY CASE WHEN exact_fingerprint IS NULL THEN 'path:'||path ELSE 'sha256:'||fingerprint_size||':'||exact_fingerprint END
              ORDER BY {query.OrderBy}) AS duplicate_rank
          FROM filtered_tracks
        )
        """;

    /// <summary>
    /// Checks current filesystem metadata once per snapshot, then reuses the indexed hashes
    /// for later searches. Store writes invalidate immediately; a 30-minute maximum age
    /// also catches files changed outside the app even if no library scan was requested.
    /// </summary>
    public void PrepareExactFingerprints(CancellationToken cancellationToken = default)
    {
        lock (_fingerprintRefreshGate)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (_fingerprintSnapshotCurrent && DateTime.UtcNow - _fingerprintSnapshotValidatedUtc < FingerprintSnapshotMaxAge)
            {
                if (_unavailableFingerprintPaths.Count == 0) return;
                if (DateTime.UtcNow - _unavailableFingerprintRetryUtc < UnavailableFingerprintRetryInterval) return;
                var unavailablePathReturned = _unavailableFingerprintPaths.Any(path =>
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    try { return TryGetFileState(path, out _, out _); }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException) { return false; }
                });
                _unavailableFingerprintRetryUtc = DateTime.UtcNow;
                if (!unavailablePathReturned) return;
                InvalidateFingerprintState();
            }
            var generation = Interlocked.Read(ref _fingerprintGeneration);
            using var connection = Open();
            using (var create = connection.CreateCommand())
            {
                create.CommandText = """
                    DROP TABLE IF EXISTS temp.current_fingerprint_state;
                    CREATE TEMP TABLE current_fingerprint_state(
                      path TEXT PRIMARY KEY,file_size INTEGER NULL,modified_utc TEXT NULL,available INTEGER NOT NULL,
                      cached_file_size INTEGER NULL,cached_modified_utc TEXT NULL,cached_sha256 TEXT NULL) WITHOUT ROWID;
                    CREATE INDEX ix_current_fingerprint_state_size ON current_fingerprint_state(available,file_size);
                    """;
                create.ExecuteNonQuery();
            }

            _unavailableFingerprintPaths = StageCurrentFileStates(connection, cancellationToken);
            using (var repeatedSizes = connection.CreateCommand())
            {
                repeatedSizes.CommandText = """
                    DROP TABLE IF EXISTS temp.current_fingerprint_repeated_sizes;
                    CREATE TEMP TABLE current_fingerprint_repeated_sizes(file_size INTEGER PRIMARY KEY) WITHOUT ROWID;
                    INSERT INTO current_fingerprint_repeated_sizes(file_size)
                    SELECT file_size FROM current_fingerprint_state
                    WHERE available=1 GROUP BY file_size HAVING COUNT(*)>1;
                    """;
                repeatedSizes.ExecuteNonQuery();
            }
            ProcessFingerprintWork(connection, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            _fingerprintSnapshotCurrent = generation == Interlocked.Read(ref _fingerprintGeneration);
            _fingerprintSnapshotValidatedUtc = DateTime.UtcNow;
            _unavailableFingerprintRetryUtc = DateTime.UtcNow;
        }
    }

    internal void InvalidateExactFingerprintSnapshot()
    {
        InvalidateFingerprintState();
    }

    private List<string> StageCurrentFileStates(SqliteConnection connection, CancellationToken cancellationToken)
    {
        string? afterPath = null;
        var inaccessible = 0;
        var unavailablePaths = new List<string>();
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var candidates = ReadFingerprintCandidateBatch(connection, afterPath);
            if (candidates.Count == 0) break;
            var states = new List<(FingerprintCandidate Candidate, long? Size, string? Modified, bool Available)>(candidates.Count);
            foreach (var candidate in candidates)
            {
                cancellationToken.ThrowIfCancellationRequested();
                try
                {
                    if (TryGetFileState(candidate.Path, out var size, out var modified))
                        states.Add((candidate, size, Stamp(modified), true));
                    else
                    {
                        inaccessible++;
                        unavailablePaths.Add(candidate.Path);
                        states.Add((candidate, null, null, false));
                    }
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException)
                {
                    inaccessible++;
                    unavailablePaths.Add(candidate.Path);
                    states.Add((candidate, null, null, false));
                }
            }

            using var transaction = connection.BeginTransaction();
            using var insert = connection.CreateCommand();
            insert.Transaction = transaction;
            insert.CommandText = "INSERT INTO current_fingerprint_state(path,file_size,modified_utc,available,cached_file_size,cached_modified_utc,cached_sha256) VALUES($path,$size,$modified,$available,$cachedSize,$cachedModified,$hash)";
            var path = insert.Parameters.Add("$path", SqliteType.Text);
            var sizeParameter = insert.Parameters.Add("$size", SqliteType.Integer);
            var modifiedParameter = insert.Parameters.Add("$modified", SqliteType.Text);
            var available = insert.Parameters.Add("$available", SqliteType.Integer);
            var cachedSize = insert.Parameters.Add("$cachedSize", SqliteType.Integer);
            var cachedModified = insert.Parameters.Add("$cachedModified", SqliteType.Text);
            var hash = insert.Parameters.Add("$hash", SqliteType.Text);
            insert.Prepare();
            foreach (var state in states)
            {
                cancellationToken.ThrowIfCancellationRequested();
                path.Value = state.Candidate.Path;
                sizeParameter.Value = state.Size.HasValue ? state.Size.Value : DBNull.Value;
                modifiedParameter.Value = state.Modified is null ? DBNull.Value : state.Modified;
                available.Value = state.Available ? 1 : 0;
                cachedSize.Value = state.Candidate.CachedSize.HasValue ? state.Candidate.CachedSize.Value : DBNull.Value;
                cachedModified.Value = state.Candidate.CachedModified is null ? DBNull.Value : state.Candidate.CachedModified;
                hash.Value = state.Candidate.CachedSha256 is null ? DBNull.Value : state.Candidate.CachedSha256;
                insert.ExecuteNonQuery();
            }
            transaction.Commit();
            afterPath = candidates[^1].Path;
        }
        if (inaccessible > 0)
            _log.Warning("duplicates", $"Could not inspect {inaccessible:N0} indexed file(s); they are excluded from verified duplicate groups.");
        return unavailablePaths;
    }

    private static List<FingerprintCandidate> ReadFingerprintCandidateBatch(SqliteConnection connection, string? afterPath)
    {
        using var command = connection.CreateCommand();
        command.CommandText = afterPath is null ? """
            SELECT t.path,f.file_size,f.modified_utc,f.sha256
            FROM tracks t LEFT JOIN track_fingerprints f ON f.path=t.path
            ORDER BY t.path LIMIT $limit
            """ : """
            SELECT t.path,f.file_size,f.modified_utc,f.sha256
            FROM tracks t LEFT JOIN track_fingerprints f ON f.path=t.path
            WHERE t.path>$after
            ORDER BY t.path LIMIT $limit
            """;
        if (afterPath is not null) command.Parameters.AddWithValue("$after", afterPath);
        command.Parameters.AddWithValue("$limit", FingerprintStateBatchSize);
        using var reader = command.ExecuteReader();
        var candidates = new List<FingerprintCandidate>(FingerprintStateBatchSize);
        while (reader.Read())
            candidates.Add(new(reader.GetString(0), reader.IsDBNull(1) ? null : reader.GetInt64(1),
                reader.IsDBNull(2) ? null : reader.GetString(2), reader.IsDBNull(3) ? null : reader.GetString(3)));
        return candidates;
    }

    private void ProcessFingerprintWork(SqliteConnection connection, CancellationToken cancellationToken)
    {
        string? afterPath = null;
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var batch = ReadFingerprintWorkBatch(connection, afterPath);
            if (batch.Count == 0) break;
            var updates = new List<FingerprintUpdate>(batch.Count);
            foreach (var item in batch)
            {
                if (cancellationToken.IsCancellationRequested)
                {
                    WriteFingerprintUpdates(connection, updates);
                    cancellationToken.ThrowIfCancellationRequested();
                }
                if (!item.Available)
                {
                    updates.Add(new(item.Path, null, null, null));
                    continue;
                }

                try
                {
                    var size = item.Size!.Value;
                    var modified = ParseStamp(item.Modified!);
                    var digest = HashFile(item.Path, cancellationToken);
                    if (!TryGetFileState(item.Path, out var verifiedSize, out var verifiedModified) ||
                        verifiedSize != size || verifiedModified != modified)
                    {
                        // A changing file is not safe to group. A later request will re-check it.
                        updates.Add(new(item.Path, null, null, null));
                        continue;
                    }
                    updates.Add(new(item.Path, size, Stamp(modified), digest));
                }
                catch (OperationCanceledException)
                {
                    WriteFingerprintUpdates(connection, updates);
                    throw;
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException or FormatException)
                {
                    // An inaccessible or changing file cannot be reported as a verified duplicate.
                    if (item.CachedSha256 is not null) updates.Add(new(item.Path, null, null, null));
                    _log.Warning("duplicates", $"Could not verify duplicate fingerprint for '{item.Path}'.", ex);
                }
            }

            WriteFingerprintUpdates(connection, updates);
            afterPath = batch[^1].Path;
        }
    }

    private static List<FingerprintWorkItem> ReadFingerprintWorkBatch(SqliteConnection connection, string? afterPath)
    {
        using var command = connection.CreateCommand();
        command.CommandText = afterPath is null ? """
            SELECT c.path,c.available,c.file_size,c.modified_utc,c.cached_file_size,c.cached_modified_utc,c.cached_sha256
            FROM current_fingerprint_state c
            WHERE (
              (c.available=0 AND c.cached_sha256 IS NOT NULL) OR
              (c.available=1 AND (
                EXISTS(SELECT 1 FROM current_fingerprint_repeated_sizes s WHERE s.file_size=c.file_size)
                OR c.cached_sha256 IS NOT NULL) AND (
                  c.cached_sha256 IS NULL OR c.cached_file_size<>c.file_size OR c.cached_modified_utc<>c.modified_utc))
            )
            ORDER BY c.path LIMIT $limit
            """ : """
            SELECT c.path,c.available,c.file_size,c.modified_utc,c.cached_file_size,c.cached_modified_utc,c.cached_sha256
            FROM current_fingerprint_state c
            WHERE c.path>$after AND (
              (c.available=0 AND c.cached_sha256 IS NOT NULL) OR
              (c.available=1 AND (
                EXISTS(SELECT 1 FROM current_fingerprint_repeated_sizes s WHERE s.file_size=c.file_size)
                OR c.cached_sha256 IS NOT NULL) AND (
                  c.cached_sha256 IS NULL OR c.cached_file_size<>c.file_size OR c.cached_modified_utc<>c.modified_utc))
            )
            ORDER BY c.path LIMIT $limit
            """;
        if (afterPath is not null) command.Parameters.AddWithValue("$after", afterPath);
        command.Parameters.AddWithValue("$limit", FingerprintHashBatchSize);
        using var reader = command.ExecuteReader();
        var items = new List<FingerprintWorkItem>(FingerprintHashBatchSize);
        while (reader.Read()) items.Add(new(reader.GetString(0), reader.GetInt64(1) != 0,
            reader.IsDBNull(2) ? null : reader.GetInt64(2), reader.IsDBNull(3) ? null : reader.GetString(3),
            reader.IsDBNull(4) ? null : reader.GetInt64(4), reader.IsDBNull(5) ? null : reader.GetString(5),
            reader.IsDBNull(6) ? null : reader.GetString(6)));
        return items;
    }

    private static void WriteFingerprintUpdates(SqliteConnection connection, IReadOnlyList<FingerprintUpdate> updates)
    {
        if (updates.Count == 0) return;
        using var transaction = connection.BeginTransaction();
        using var delete = connection.CreateCommand();
        delete.Transaction = transaction; delete.CommandText = "DELETE FROM track_fingerprints WHERE path=$path";
        var deletePath = delete.Parameters.Add("$path", SqliteType.Text); delete.Prepare();
        using var upsert = connection.CreateCommand();
        upsert.Transaction = transaction;
        // SELECT/EXISTS makes deletion of a track between filesystem reading and cache write harmless.
        upsert.CommandText = "INSERT OR REPLACE INTO track_fingerprints(path,file_size,modified_utc,sha256) SELECT $path,$size,$modified,$hash WHERE EXISTS(SELECT 1 FROM tracks WHERE path=$path)";
        var pathParameter = upsert.Parameters.Add("$path", SqliteType.Text);
        var sizeParameter = upsert.Parameters.Add("$size", SqliteType.Integer);
        var modifiedParameter = upsert.Parameters.Add("$modified", SqliteType.Text);
        var hashParameter = upsert.Parameters.Add("$hash", SqliteType.Text);
        upsert.Prepare();
        foreach (var update in updates)
        {
            if (update.Sha256 is null)
            {
                deletePath.Value = update.Path; delete.ExecuteNonQuery();
                continue;
            }
            pathParameter.Value = update.Path; sizeParameter.Value = update.Size!.Value;
            modifiedParameter.Value = update.Modified!; hashParameter.Value = update.Sha256;
            upsert.ExecuteNonQuery();
        }
        transaction.Commit();
    }

    private static bool TryGetFileState(string path, out long size, out DateTime modifiedUtc)
    {
        var info = new FileInfo(path); info.Refresh();
        size = info.Exists ? info.Length : 0;
        modifiedUtc = info.Exists ? info.LastWriteTimeUtc : default;
        return info.Exists;
    }

    private static string HashFile(string path, CancellationToken cancellationToken)
    {
        const int bufferSize = 1024 * 1024;
        var buffer = ArrayPool<byte>.Shared.Rent(bufferSize);
        try
        {
            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read,
                bufferSize, FileOptions.SequentialScan);
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            int read;
            while ((read = stream.Read(buffer, 0, bufferSize)) > 0)
            {
                cancellationToken.ThrowIfCancellationRequested();
                hash.AppendData(buffer, 0, read);
            }
            cancellationToken.ThrowIfCancellationRequested();
            return Convert.ToHexString(hash.GetHashAndReset());
        }
        finally { ArrayPool<byte>.Shared.Return(buffer); }
    }

    private static void ValidatePage(int offset, int pageSize, string itemName)
    {
        if (offset < 0) throw new ArgumentOutOfRangeException(nameof(offset));
        if (pageSize is < 1 or > 800) throw new ArgumentOutOfRangeException(nameof(pageSize), $"Page size must be between 1 and 800 {itemName}.");
    }
}
