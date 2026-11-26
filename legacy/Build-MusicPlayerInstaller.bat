@echo off
setlocal
where pwsh.exe >nul 2>&1
if errorlevel 1 (
    echo PowerShell 7 is required for the local packaging tools.
    exit /b 1
)
pwsh.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0packaging\windows\Build-LocalInstaller.ps1" %*
if errorlevel 1 (
    echo Local installer build failed. See the error above.
    exit /b 1
)
exit /b 0
