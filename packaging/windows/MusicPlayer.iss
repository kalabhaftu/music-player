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
VersionInfoVersion={#AppVersion}.0
VersionInfoProductVersion={#AppVersion}.0
AppPublisher=Kalabhaftu
AppPublisherURL=https://github.com/kalabhaftu/music-player
AppSupportURL=https://github.com/kalabhaftu/music-player/issues
AppUpdatesURL=https://github.com/kalabhaftu/music-player/releases
VersionInfoCompany=Kalabhaftu
VersionInfoProductName=Music Player
VersionInfoDescription=Music Player Installer (Native Rust)
VersionInfoOriginalFileName=MusicPlayer-Setup-{#Architecture}.exe
DefaultDirName={autopf}\Music Player
DefaultGroupName=Music Player
UninstallDisplayIcon={app}\MusicPlayer.exe
ArchitecturesAllowed={#Architecture}
ArchitecturesInstallIn64BitMode={#Architecture}
PrivilegesRequired=lowest
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
AppMutex=Local\MusicPlayer.SingleInstance
ChangesAssociations=yes
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
Source: "{#SourceDir}\*"; DestDir: "{app}"; Excludes: "Portable-README.txt,Uninstall-MusicPlayer.ps1,*.pdb,*.d,*.rlib,.cargo-*"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Music Player"; Filename: "{app}\MusicPlayer.exe"; IconFilename: "{app}\Assets\MusicPlayer.ico"; Comment: "Offline music library and player"
Name: "{autodesktop}\Music Player"; Filename: "{app}\MusicPlayer.exe"; IconFilename: "{app}\Assets\MusicPlayer.ico"; Comment: "Offline music library and player"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Registry]
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\MusicPlayer.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\MusicPlayer.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\MusicPlayer.exe"; ValueType: string; ValueName: "Path"; ValueData: "{app}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\music-player-app.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\MusicPlayer.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\music-player-app.exe"; ValueType: string; ValueName: "Path"; ValueData: "{app}"; Flags: uninsdeletekey

[Run]
Filename: "{app}\MusicPlayer.exe"; Parameters: "--register-associations"; Flags: runhidden waituntilterminated
Filename: "{app}\MusicPlayer.exe"; Description: "{cm:LaunchProgram,Music Player}"; Flags: postinstall nowait skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{app}\MusicPlayer.exe"; Parameters: "--unregister-associations"; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterAssociations"

[Code]
var
  RemoveMusicPlayerData: Boolean;

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
    if not DelTree(ExpandConstant('{localappdata}\MusicPlayer'), True, True, True) then
      MsgBox('Some saved Music Player data could not be removed. Close Music Player and remove its folder in Local AppData manually.', mbError, MB_OK);
  end;
end;
