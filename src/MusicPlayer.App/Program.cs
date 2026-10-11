using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using MusicPlayer.Core;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel.Activation;

namespace MusicPlayer.App;

internal static class Program
{
    private const string InstanceKey = "music-player-main";
    private const string ProcessMutexName = "Local\\MusicPlayer-main-process";
    private const string SecondaryActivationName = "Local\\MusicPlayer-main-activate";
    private static readonly object ActivationGate = new();
    private static readonly Queue<FileActivationRequest> PendingActivations = new();
    private static AppInstance? _instance;
    private static Mutex? _processMutex;
    private static EventWaitHandle? _secondaryActivation;
    private static DispatcherQueue? _dispatcherQueue;
    private static App? _app;

    internal static FileActivationRequest InitialActivation { get; private set; } = new(FileActivationAction.Play, []);

    [STAThread]
    private static void Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();

        var current = AppInstance.GetCurrent();
        var activation = current.GetActivatedEventArgs();
        var instance = AppInstance.FindOrRegisterForKey(InstanceKey);
        if (!instance.IsCurrent)
        {
            if (activation is not null)
            {
                try { RedirectActivationAsync(instance, activation).GetAwaiter().GetResult(); }
                catch (Exception ex) { LocalAppLog.Shared.Error("activation", "Could not forward an activation to the running Music Player window.", ex); }
            }
            return;
        }

        // AppInstance scopes can differ between packaged and unpackaged launches.
        // A named mutex also prevents two output/install copies from scanning and
        // writing the same per-user library at once.
        _processMutex = new Mutex(false, ProcessMutexName);
        var ownsProcessMutex = false;
        try { ownsProcessMutex = _processMutex.WaitOne(0); }
        catch (AbandonedMutexException) { ownsProcessMutex = true; }
        if (!ownsProcessMutex)
        {
            _processMutex.Dispose();
            _processMutex = null;
            SignalExistingWindow(ParseActivation(activation, args));
            LocalAppLog.Shared.Info("activation", "A second Music Player process was redirected to the existing window.");
            return;
        }

        _secondaryActivation = new EventWaitHandle(false, EventResetMode.AutoReset, SecondaryActivationName);

        _instance = instance;
        InitialActivation = ParseActivation(activation, args);
        instance.Activated += Instance_Activated;
        ListenForSecondaryActivation();

        Application.Start(_ =>
        {
            var dispatcher = DispatcherQueue.GetForCurrentThread();
            SynchronizationContext.SetSynchronizationContext(new DispatcherQueueSynchronizationContext(dispatcher));
            Attach(new App(), dispatcher);
        });
    }

    private static async Task RedirectActivationAsync(AppInstance instance, AppActivationArguments activation)
        => await instance.RedirectActivationToAsync(activation);

    private static void SignalExistingWindow(FileActivationRequest request)
    {
        string? requestFile = null;
        try
        {
            var activationDirectory = Path.Combine(AppDataPaths.Root, "Activations");
            Directory.CreateDirectory(activationDirectory);
            requestFile = Path.Combine(activationDirectory, $"{Guid.NewGuid():N}.json");
            File.WriteAllText(requestFile, JsonSerializer.Serialize(request), new UTF8Encoding(false));
            using var signal = EventWaitHandle.OpenExisting(SecondaryActivationName);
            signal.Set();
        }
        catch (WaitHandleCannotBeOpenedException)
        {
            // The existing process may be exiting; its mutex will be released by Windows.
            if (requestFile is not null) try { File.Delete(requestFile); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            if (requestFile is not null) try { File.Delete(requestFile); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            LocalAppLog.Shared.Warning("activation", "Could not signal the existing Music Player window.", ex);
        }
    }

    private static void ListenForSecondaryActivation()
    {
        var signal = _secondaryActivation;
        if (signal is null) return;
        _ = Task.Run(() =>
        {
            while (true)
            {
                try { signal.WaitOne(); }
                catch (ObjectDisposedException) { return; }

                var activationDirectory = Path.Combine(AppDataPaths.Root, "Activations");
                if (!Directory.Exists(activationDirectory)) continue;
                foreach (var requestFile in Directory.EnumerateFiles(activationDirectory, "*.json").OrderBy(path => path, StringComparer.Ordinal))
                {
                    try
                    {
                        var json = File.ReadAllText(requestFile, Encoding.UTF8);
                        var request = JsonSerializer.Deserialize<FileActivationRequest>(json);
                        if (request is null) continue;
                        App? app;
                        DispatcherQueue? dispatcher;
                        lock (ActivationGate)
                        {
                            app = _app;
                            dispatcher = _dispatcherQueue;
                            if (app is null || dispatcher is null)
                            {
                                PendingActivations.Enqueue(request);
                                continue;
                            }
                        }
                        if (!dispatcher.TryEnqueue(() => app.HandleActivation(request)))
                            LocalAppLog.Shared.Warning("activation", "The UI dispatcher could not process an activation from a second process.");
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException)
                    { LocalAppLog.Shared.Warning("activation", "Could not read a forwarded activation request.", ex); }
                    finally { try { File.Delete(requestFile); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
                }
            }
        });
    }

    private static void Instance_Activated(object? sender, AppActivationArguments activation)
    {
        var request = ParseActivation(activation, []);
        App? app;
        DispatcherQueue? dispatcher;
        lock (ActivationGate)
        {
            app = _app;
            dispatcher = _dispatcherQueue;
            if (app is null || dispatcher is null)
            {
                PendingActivations.Enqueue(request);
                return;
            }
        }

        if (!dispatcher.TryEnqueue(() => app.HandleActivation(request)))
            LocalAppLog.Shared.Warning("activation", "The Music Player UI dispatcher was not available to process an activation.");
    }

    private static void Attach(App app, DispatcherQueue dispatcher)
    {
        FileActivationRequest[] pending;
        lock (ActivationGate)
        {
            _app = app;
            _dispatcherQueue = dispatcher;
            pending = PendingActivations.ToArray();
            PendingActivations.Clear();
        }

        foreach (var request in pending)
            dispatcher.TryEnqueue(() => app.HandleActivation(request));
    }

    private static FileActivationRequest ParseActivation(AppActivationArguments? activation, string[] commandLineArguments)
    {
        var arguments = commandLineArguments.Length > 0
            ? commandLineArguments
            : activation?.Data is ILaunchActivatedEventArgs launch ? ParseArguments(launch.Arguments) : [];
        if (activation?.Kind == ExtendedActivationKind.File && activation.Data is IFileActivatedEventArgs files)
            return FileActivation.Parse(arguments, files.Verb, files.Files.Select(file => file.Path));
        return FileActivation.Parse(arguments);
    }

    private static string[] ParseArguments(string arguments)
    {
        if (string.IsNullOrWhiteSpace(arguments)) return [];
        var commandLine = CommandLineToArgvW("MusicPlayer.exe " + arguments, out var count);
        if (commandLine == 0) return [];
        try
        {
            var parsed = new string[Math.Max(0, count - 1)];
            for (var index = 1; index < count; index++)
                parsed[index - 1] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(commandLine, index * IntPtr.Size)) ?? string.Empty;
            return parsed;
        }
        finally { _ = LocalFree(commandLine); }
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern nint CommandLineToArgvW(string commandLine, out int argumentCount);

    [DllImport("kernel32.dll")]
    private static extern nint LocalFree(nint memory);
}
