using System.Security.Cryptography;
using System.Text;

namespace MusicPlayer.Core;

internal sealed class FilePathLock : IDisposable
{
    private readonly Mutex _mutex;

    public FilePathLock(string path)
    {
        var canonical = Path.GetFullPath(path);
        if (OperatingSystem.IsWindows()) canonical = canonical.ToUpperInvariant();
        var name = "MusicPlayer.FileWrite." + Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(canonical)));
        _mutex = new Mutex(false, name);
        try { _mutex.WaitOne(); }
        catch (AbandonedMutexException) { } // Ownership transfers to this process; the previous writer has exited.
        catch { _mutex.Dispose(); throw; }
    }

    public void Dispose()
    {
        _mutex.ReleaseMutex();
        _mutex.Dispose();
    }
}
