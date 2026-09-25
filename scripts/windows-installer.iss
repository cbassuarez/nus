; nus per-user Windows installer. package-release.py --build-installer compiles
; it from the signed stage, so it installs exactly the portable ZIP's folder.
; Each channel has one fixed, user-writable folder: the in-app updater swaps
; that folder in place, and nus keeps one profile for it (installed-location).
;   ISCC /DVersion=0.0.1-preview.9 /DNumericVersion=0.0.1 /DChannel=preview
;        /DSourceDir=... /DOutputDir=... /DOutputName=... windows-installer.iss

#if Channel == "preview"
  ; Never change an AppId: it is how upgrades find the existing installation.
  #define AppIdGuid "{{74ED99B0-ACEA-4860-A2AA-86FD0B170CFB}"
  #define AppName "nus Preview"
#elif Channel == "release"
  #define AppIdGuid "{{E991BB0F-52B1-4A72-ABE7-395558E71B60}"
  #define AppName "nus"
#else
  #error Channel must be preview or release
#endif

[Setup]
AppId={#AppIdGuid}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=nus
AppPublisherURL=https://cbassuarez.com/nus.dev/
AppSupportURL=https://github.com/cbassuarez/nus/issues
AppUpdatesURL=https://cbassuarez.com/nus.dev/download/
VersionInfoVersion={#NumericVersion}
; No elevation, and no choice of folder: the updater and the profile both
; rely on this exact location, and uninstall removes it whole.
PrivilegesRequired=lowest
DefaultDirName={userpf}\nus\{#Channel}
DisableDirPage=yes
DisableProgramGroupPage=yes
; Outside {app}, so the updater's folder swap cannot take the uninstaller with it.
UninstallFilesDir={localappdata}\nus\uninstall\{#Channel}
UninstallDisplayIcon={app}\nus.exe
UninstallDisplayName={#AppName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile={#SourcePath}..\assets\icon\nus.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes
SetupLogging=yes
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
Name: "path"; Description: "Add the nus shell command (bin\nus.exe) to PATH"; Flags: unchecked

[InstallDelete]
; Like an in-app update, an upgrade replaces the whole package: no runtime
; file from an older version survives beside the new one.
Type: filesandordirs; Name: "{app}\*"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\nus.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\nus.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\nus.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The folder may hold files an in-app update added after installation.
; Profiles and settings under %LOCALAPPDATA%\nus\installs are kept.
Type: filesandordirs; Name: "{app}"
Type: files; Name: "{localappdata}\nus\installs\{#Channel}\installed-location"

[Code]
const
  EnvironmentKey = 'Environment';

function CommandFolder: String;
begin
  Result := ExpandConstant('{app}\bin');
end;

procedure AddToPath;
var
  Path: String;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Path) then Path := '';
  if Pos(';' + Uppercase(CommandFolder) + ';', ';' + Uppercase(Path) + ';') > 0 then exit;
  if (Path <> '') and (Path[Length(Path)] <> ';') then Path := Path + ';';
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Path + CommandFolder);
end;

procedure RemoveFromPath;
var
  Path: String;
  At: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Path) then exit;
  Path := ';' + Path + ';';
  At := Pos(';' + Uppercase(CommandFolder) + ';', Uppercase(Path));
  if At = 0 then exit;
  Delete(Path, At, Length(CommandFolder) + 1);
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Copy(Path, 2, Length(Path) - 2));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Channel: String;
begin
  if CurStep <> ssPostInstall then exit;
  if WizardIsTaskSelected('path') then AddToPath else RemoveFromPath;
  // nus reads this to recognize its installed copy (install.rs installed_here).
  Channel := ExpandConstant('{localappdata}\nus\installs\{#Channel}');
  ForceDirectories(Channel);
  SaveStringToFile(Channel + '\installed-location', ExpandConstant('{app}'), False);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then RemoveFromPath;
end;
