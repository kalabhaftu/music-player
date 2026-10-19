using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;
using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

// Explorer supplies the complete selection as an IDataObject. A command's %1
// only supplies one file; %* does not turn it into a selection array.
public static class ShellDropTargetServer
{
    internal const string PlayClass = "A3B79A2E-C313-45F7-BB02-F9F4B0E4C101";
    internal const string QueueClass = "A3B79A2E-C313-45F7-BB02-F9F4B0E4C102";
    internal const string PlaylistClass = "A3B79A2E-C313-45F7-BB02-F9F4B0E4C103";
    private static long _lastActivity = Stopwatch.GetTimestamp();
    private static int _serverLocks;

    internal static void Run()
    {
        Marshal.ThrowExceptionForHR(OleInitialize(0));
        var registrations = new List<uint>();
        var factories = new List<ClassFactory>();
        nuint timer = 0;
        try
        {
            foreach (var (id, action) in new[] { (PlayClass, FileActivationAction.Play), (QueueClass, FileActivationAction.AddToQueue), (PlaylistClass, FileActivationAction.CreatePlaylist) })
            {
                var clsid = new Guid(id);
                var factory = new ClassFactory(action);factories.Add(factory);
                Marshal.ThrowExceptionForHR(CoRegisterClassObject(ref clsid, factory, 4, 1, out var cookie));
                registrations.Add(cookie);
            }
            timer = SetTimer(0, 0, 1000, 0);
            if (timer == 0) throw new InvalidOperationException("Could not start the Explorer action server.");
            while (GetMessage(out var message, 0, 0, 0) > 0)
            {
                if (message.Message == 0x113 && Volatile.Read(ref _serverLocks) == 0 && Stopwatch.GetElapsedTime(_lastActivity) > TimeSpan.FromMinutes(5)) break;
                TranslateMessage(ref message);DispatchMessage(ref message);
            }
        }
        catch (Exception ex) { LocalAppLog.Shared.Error("shell-selection", "Could not process the Explorer selection.", ex); }
        finally
        {
            if (timer != 0) KillTimer(0, timer);
            foreach (var cookie in registrations) CoRevokeClassObject(cookie);
            GC.KeepAlive(factories);OleUninitialize();
        }
    }

    internal static FileActivationRequest? ReadRequest(string token)
    {
        if (!Guid.TryParseExact(token, "N", out var id)) return null;
        var path = Path.Combine(AppDataPaths.Root, "ShellRequests", id.ToString("N") + ".json");
        try
        {
            var request = JsonSerializer.Deserialize<FileActivationRequest>(File.ReadAllText(path));
            return request is null ? null : FileActivation.Parse([], request.Action.ToString(), request.Paths);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException)
        { LocalAppLog.Shared.Warning("shell-selection", "Could not read an Explorer selection.", ex);return null; }
        finally { try { File.Delete(path); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
    }

    [ComVisible(true), Guid("00000122-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IDropTarget
    {
        [PreserveSig] int DragEnter([MarshalAs(UnmanagedType.Interface)] IDataObject data, uint keys, Point point, ref uint effect);
        [PreserveSig] int DragOver(uint keys, Point point, ref uint effect);
        [PreserveSig] int DragLeave();
        [PreserveSig] int Drop([MarshalAs(UnmanagedType.Interface)] IDataObject data, uint keys, Point point, ref uint effect);
    }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    public sealed class DropTarget(FileActivationAction action) : IDropTarget
    {
        public int DragEnter(IDataObject data, uint keys, Point point, ref uint effect) { _lastActivity=Stopwatch.GetTimestamp();effect=1;return 0; }
        public int DragOver(uint keys, Point point, ref uint effect) { _lastActivity=Stopwatch.GetTimestamp();effect=1;return 0; }
        public int DragLeave() { _lastActivity=Stopwatch.GetTimestamp();return 0; }
        public int Drop(IDataObject data, uint keys, Point point, ref uint effect)
        {
            _lastActivity=Stopwatch.GetTimestamp();effect=0;
            var format = new FORMATETC { cfFormat=15, dwAspect=DVASPECT.DVASPECT_CONTENT, lindex=-1, tymed=TYMED.TYMED_HGLOBAL };
            STGMEDIUM medium=default;
            var received=false;
            try
            {
                data.GetData(ref format, out medium);received=true;
                var count=DragQueryFile(medium.unionmember,uint.MaxValue,null,0);
                var paths=new List<string>((int)count);
                for(uint index=0;index<count;index++)
                {
                    var buffer=new StringBuilder((int)DragQueryFile(medium.unionmember,index,null,0)+1);
                    DragQueryFile(medium.unionmember,index,buffer,(uint)buffer.Capacity);paths.Add(buffer.ToString());
                }
                var request=FileActivation.Parse([],action.ToString(),paths);
                if(request.Paths.Count==0)return 0;
                var directory=Path.Combine(AppDataPaths.Root,"ShellRequests");Directory.CreateDirectory(directory);
                var token=Guid.NewGuid().ToString("N");var path=Path.Combine(directory,token+".json");
                File.WriteAllText(path,JsonSerializer.Serialize(request));
                var start=new ProcessStartInfo(Environment.ProcessPath!) { UseShellExecute=false, CreateNoWindow=true };
                start.ArgumentList.Add("--file-request");start.ArgumentList.Add(token);
                try { using var process=Process.Start(start) ?? throw new IOException("Could not launch Music Player."); }
                catch { File.Delete(path);throw; }
                effect=1;return 0;
            }
            catch(Exception ex) { LocalAppLog.Shared.Error("shell-selection","The selected files could not be sent to Music Player.",ex);return Marshal.GetHRForException(ex); }
            finally { if(received)ReleaseStgMedium(ref medium); }
        }
    }

    [ComVisible(true), Guid("00000001-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IClassFactory
    {
        [PreserveSig] int CreateInstance(nint outer, ref Guid iid, out nint instance);
        [PreserveSig] int LockServer([MarshalAs(UnmanagedType.Bool)] bool locked);
    }
    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    public sealed class ClassFactory(FileActivationAction action) : IClassFactory
    {
        public int CreateInstance(nint outer,ref Guid iid,out nint instance)
        {
            _lastActivity=Stopwatch.GetTimestamp();instance=0;if(outer!=0)return unchecked((int)0x80040110);
            var unknown=Marshal.GetIUnknownForObject(new DropTarget(action));
            try { return Marshal.QueryInterface(unknown,in iid,out instance); } finally { Marshal.Release(unknown); }
        }
        public int LockServer(bool locked) { _lastActivity=Stopwatch.GetTimestamp();if(locked)Interlocked.Increment(ref _serverLocks);else if(Volatile.Read(ref _serverLocks)>0)Interlocked.Decrement(ref _serverLocks);return 0; }
    }
    [StructLayout(LayoutKind.Sequential)] private struct MessageData { public nint Window; public uint Message; public nuint WParam; public nint LParam; public uint Time; public Point Point; public uint Private; }
    [DllImport("ole32.dll")] private static extern int OleInitialize(nint reserved);
    [DllImport("ole32.dll")] private static extern void OleUninitialize();
    [DllImport("ole32.dll")] private static extern int CoRegisterClassObject(ref Guid clsid,[MarshalAs(UnmanagedType.Interface)] IClassFactory factory,uint context,uint flags,out uint cookie);
    [DllImport("ole32.dll")] private static extern int CoRevokeClassObject(uint cookie);
    [DllImport("ole32.dll")] private static extern void ReleaseStgMedium(ref STGMEDIUM medium);
    [DllImport("shell32.dll",CharSet=CharSet.Unicode,EntryPoint="DragQueryFileW")] private static extern uint DragQueryFile(nint drop,uint index,StringBuilder? path,uint size);
    [DllImport("user32.dll")] private static extern nuint SetTimer(nint window,nuint id,uint interval,nint callback);
    [DllImport("user32.dll")] [return:MarshalAs(UnmanagedType.Bool)] private static extern bool KillTimer(nint window,nuint id);
    [DllImport("user32.dll",EntryPoint="GetMessageW")] private static extern int GetMessage(out MessageData message,nint window,uint minimum,uint maximum);
    [DllImport("user32.dll")] [return:MarshalAs(UnmanagedType.Bool)] private static extern bool TranslateMessage(ref MessageData message);
    [DllImport("user32.dll",EntryPoint="DispatchMessageW")] private static extern nint DispatchMessage(ref MessageData message);
}
