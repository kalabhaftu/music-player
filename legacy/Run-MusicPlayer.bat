@echo off
setlocal
cd /d "%~dp0"

set "PROJECT=src\MusicPlayer.App\MusicPlayer.App.csproj"
set "APP_EXE=%~dp0artifacts\launcher\x64\Release\net10.0-windows10.0.26100.0\win-x64\MusicPlayer.exe"

if not exist "%PROJECT%" (
    echo Could not find the Music Player project. Run this file from the repository checkout.
    goto :failed
)

where dotnet >nul 2>&1
if errorlevel 1 (
    echo .NET SDK was not found. Install the .NET 10 SDK, then run this file again.
    goto :failed
)

echo Restoring Windows x64 dependencies...
echo NU1900 means NuGet could not reach its vulnerability feed. Auditing remains enabled.
dotnet restore "%PROJECT%" --runtime win-x64 -p:Platform=x64
if errorlevel 1 (
    echo Restore failed. Retrying once in case the feed was temporarily unavailable...
    dotnet restore "%PROJECT%" --runtime win-x64 -p:Platform=x64
    if errorlevel 1 goto :restore_failed
)

echo Building the latest source for Windows x64...
dotnet build "%PROJECT%" --configuration Release --runtime win-x64 --no-restore -p:Platform=x64 -p:BaseOutputPath="%~dp0artifacts/launcher/"
if errorlevel 1 goto :build_failed

if not exist "%APP_EXE%" (
    echo Build succeeded, but the expected executable was not created:
    echo "%APP_EXE%"
    goto :failed
)

echo Launching the executable from this checkout:
echo "%APP_EXE%"
echo An existing Music Player session must be closed manually before launching this build.
powershell.exe -NoLogo -NoProfile -Command "if (Get-Process -Name MusicPlayer -ErrorAction SilentlyContinue) { Write-Output 'Build completed. Music Player is already running; close it and run this file again to launch the new build.'; exit 1 }; exit 0"
if errorlevel 1 exit /b 1
echo Waiting for the app window to appear so a startup crash is reported here...
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath '%APP_EXE%' -PassThru; $deadline = (Get-Date).AddSeconds(45); do { $p.Refresh(); if ($p.HasExited) { throw ('Music Player exited during startup with code ' + $p.ExitCode + '.') }; if ($p.MainWindowHandle -ne 0) { Write-Output 'Music Player window is ready.'; exit 0 }; Start-Sleep -Milliseconds 500 } while ((Get-Date) -lt $deadline); throw 'Music Player process is running but its window did not appear within 45 seconds.'"
if errorlevel 1 goto :failed
exit /b 0

:restore_failed
echo Dependency restore failed. Check the NuGet connection and the errors above.
goto :failed

:build_failed
echo The x64 build failed. See the compiler errors above.
goto :failed

:failed
echo.
echo Music Player was not launched. Fix the message above and run this file again.
pause
exit /b 1
