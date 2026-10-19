using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public partial class App : Application
{
    private Window? _window;
    private readonly Queue<FileActivationRequest> _pendingFileActivations = new();
    private bool _drainingActivations;
    public App()
    {
        UnhandledException += (_, args) => LocalAppLog.Shared.Error("ui-crash", "Unhandled WinUI exception.", args.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, args) => LocalAppLog.Shared.Error("process-crash", "Unhandled process exception.", args.ExceptionObject as Exception ?? new InvalidOperationException(args.ExceptionObject?.ToString()));
        TaskScheduler.UnobservedTaskException += (_, args) =>
        {
            LocalAppLog.Shared.Error("background-task", "Unobserved background-task exception.", args.Exception);
            args.SetObserved();
        };
        InitializeComponent();
        LocalAppLog.Shared.Info("app", $"Starting Music Player {GetType().Assembly.GetName().Version} from '{Environment.ProcessPath}' on {System.Runtime.InteropServices.RuntimeInformation.OSDescription} ({System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture}, .NET {Environment.Version}); data profile is LocalAppData\\MusicPlayer.");
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        using var startup = StartupSplash.TryShow();
        try
        {
            // Integrity checks, migrations and recovery finish on a worker before any
            // service can write. The UI dispatcher stays available during disk work.
            var storeTask = Task.Run(LibraryStore.InAppData);
            var playbackTask = Task.Run(() => new PlaybackService());
            await Task.WhenAll(storeTask, playbackTask);
            var early = _pendingFileActivations.ToArray();_pendingFileActivations.Clear();
            var openingTrack = Program.InitialActivation.Action == FileActivationAction.Play && Program.InitialActivation.Paths.Count > 0;
            openingTrack |= early.Any(request => request.Action == FileActivationAction.Play && request.Paths.Count > 0);
            _window ??= new MainWindow(storeTask.Result, playbackTask.Result, openingTrack);
            _window.Activate();
            HandleActivation(Program.InitialActivation);
            foreach (var pending in early) HandleActivation(pending);
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("startup", "The main window failed to start.", ex);
            throw;
        }
    }

    internal void HandleActivation(FileActivationRequest request)
    {
        if (_window is null)
        {
            _pendingFileActivations.Enqueue(request);
            return;
        }

        _window.Activate();
        if (request.Paths.Count == 0) return;
        _pendingFileActivations.Enqueue(request);
        if (!_drainingActivations) _ = DrainActivationsAsync();
    }

    private async Task DrainActivationsAsync()
    {
        _drainingActivations = true;
        try
        {
            while (_pendingFileActivations.Count > 0 && _window is MainWindow mainWindow)
            {
                await Task.Delay(150);
                var request = _pendingFileActivations.Dequeue();
                var paths = request.Paths.ToList();
                while (_pendingFileActivations.TryPeek(out var next) && next.Action == request.Action)
                    paths.AddRange(_pendingFileActivations.Dequeue().Paths);
                var selected = paths.Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
                switch (request.Action)
                {
                    case FileActivationAction.AddToQueue: await mainWindow.AddExternalFilesToQueueAsync(selected); break;
                    case FileActivationAction.CreatePlaylist: await mainWindow.CreatePlaylistFromExternalFilesAsync(selected); break;
                    default: await mainWindow.PlayExternalFilesAsync(selected); break;
                }
            }
        }
        finally { _drainingActivations = false; }
    }
}
