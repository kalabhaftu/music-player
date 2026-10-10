#ifndef AppVersion
  #error AppVersion must be provided
#endif
#ifndef Architecture
  #error Architecture must be provided
#endif
#ifndef SourceDir
  #error SourceDir must be provided
#endif
#ifndef OutputDir
  #error OutputDir must be provided
#endif

[Setup]
AppId={{8B3371D0-13A1-4410-9E60-A49AD1A5F08C}
AppName=Music Player
AppVersion={#AppVersion}
AppPublisher=Kalabhaftu
DefaultDirName={autopf}\Music Player
DefaultGroupName=Music Player
UninstallDisplayIcon={app}\MusicPlayer.exe
ArchitecturesAllowed={#Architecture}
ArchitecturesInstallIn64BitMode={#Architecture}
PrivilegesRequired=lowest
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
Compression=lzma2/ultra64
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=MusicPlayer-Setup-{#Architecture}
SetupIconFile={#SourceDir}\Assets\MusicPlayer.ico
Uninstallable=yes
#ifdef SignedBuild
SignedUninstaller=yes
SignTool=musicplayer
#endif

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Excludes: "MicrosoftEdgeWebView2Setup.exe,Portable-README.txt,Uninstall-MusicPlayer.ps1"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourceDir}\MicrosoftEdgeWebView2Setup.exe"; Flags: dontcopy

[Icons]
Name: "{autoprograms}\Music Player"; Filename: "{app}\MusicPlayer.exe"
Name: "{autodesktop}\Music Player"; Filename: "{app}\MusicPlayer.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoLogo -NoProfile -ExecutionPolicy Bypass -File ""{app}\Register-MusicPlayer-FileActions.ps1"" -Quiet"; Flags: runhidden waituntilterminated
Filename: "{app}\MusicPlayer.exe"; Description: "Launch Music Player"; Flags: postinstall nowait skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoLogo -NoProfile -ExecutionPolicy Bypass -File ""{app}\Register-MusicPlayer-FileActions.ps1"" -Unregister -Quiet"; Flags: runhidden waituntilterminated

[Code]
const
  WebView2ClientKey = 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

var
  RemoveMusicPlayerData: Boolean;

function HasWebView2Runtime(RootKey: HKEY): Boolean;
var
  VersionString: String;
  Version: Int64;
begin
  Result := False;
  if not RegQueryStringValue(RootKey, WebView2ClientKey, 'pv', VersionString) then
    exit;
  if not StrToVersion(VersionString, Version) then
    exit;
  Result := Version > 0;
end;

function IsWebView2RuntimeInstalled: Boolean;
begin
  { Microsoft documents the 32-bit machine registry view and the current-user key.
    Explicit views also work when this installer runs in x64 or ARM64 mode. }
  Result := HasWebView2Runtime(HKLM32) or HasWebView2Runtime(HKCU32);
end;

procedure AppendReadySection(var Memo: String; Section, NewLine: String);
begin
  if Section = '' then
    exit;
  if Memo <> '' then
    Memo := Memo + NewLine + NewLine;
  Memo := Memo + Section;
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo,
  MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := '';
  AppendReadySection(Result, MemoUserInfoInfo, NewLine);
  AppendReadySection(Result, MemoDirInfo, NewLine);
  AppendReadySection(Result, MemoTypeInfo, NewLine);
  AppendReadySection(Result, MemoComponentsInfo, NewLine);
  AppendReadySection(Result, MemoGroupInfo, NewLine);
  AppendReadySection(Result, MemoTasksInfo, NewLine);
  if IsWebView2RuntimeInstalled then
    AppendReadySection(Result, 'Microsoft Edge WebView2 Runtime is already installed. Setup will use it without downloading or reinstalling it.', NewLine)
  else
    AppendReadySection(Result, 'Microsoft Edge WebView2 Runtime was not detected. Setup will download and install this required component using Microsoft''s installer. An internet connection is required. For offline installation, install Microsoft''s Evergreen Standalone Installer first, then run this Setup again.', NewLine);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    if UninstallSilent then
      RemoveMusicPlayerData := ExpandConstant('{param:REMOVEUSERDATA|0}') = '1'
    else
      RemoveMusicPlayerData := MsgBox(
        'Also delete Music Player''s local library index, playlists, favorites, settings, artwork cache, and logs? This does not delete your music files. Choose No to keep your library data if you may reinstall.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
  end
  else if (CurUninstallStep = usPostUninstall) and RemoveMusicPlayerData then
  begin
    { Check parameters are evaluated by Setup, not by the uninstaller. }
    if not DelTree(ExpandConstant('{localappdata}\MusicPlayer'), True, True, True) then
      MsgBox('Some saved Music Player data could not be removed. Close Music Player and remove its folder in Local AppData manually.', mbError, MB_OK);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Result := '';
  if IsWebView2RuntimeInstalled then
  begin
    Log('WebView2 Runtime already installed; skipping runtime installation.');
    exit;
  end;

  Log('WebView2 Runtime not detected; starting Microsoft runtime installer.');
  ExtractTemporaryFile('MicrosoftEdgeWebView2Setup.exe');
  if not Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebView2Setup.exe'), '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
  begin
    Result := 'Microsoft Edge WebView2 Runtime was not detected, and Setup could not start Microsoft''s runtime installer. Install the Evergreen Runtime from https://developer.microsoft.com/microsoft-edge/webview2/ and run Setup again. For offline installation, use the Evergreen Standalone Installer.';
    exit;
  end;
  Log(Format('Microsoft WebView2 runtime installer returned %d.', [ResultCode]));
  if (ResultCode = 3010) or (ResultCode = 1641) then
  begin
    NeedsRestart := True;
    Result := 'The Microsoft Edge WebView2 Runtime requires a Windows restart. Restart Windows and run Setup again.';
    exit;
  end;
  { Recheck registration even after a nonzero result: another installer may
    have installed the shared Runtime while this bootstrapper was running. }
  if IsWebView2RuntimeInstalled then
    exit;
  Result := Format('Microsoft Edge WebView2 Runtime is still missing after Microsoft''s installer returned %d. Check your internet connection, or install the Evergreen Runtime from https://developer.microsoft.com/microsoft-edge/webview2/ and run Setup again. For offline installation, use the Evergreen Standalone Installer.', [ResultCode]);
end;
