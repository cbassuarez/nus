; nus per-user Windows installer. package-release.py --build-installer compiles
; it from the signed stage, so it installs exactly the portable ZIP's folder.
; Each channel has one fixed, user-writable folder: the in-app updater swaps
; that folder in place, and nus keeps one profile for it (installed-location).
;   ISCC /DVersion=0.0.1-preview.9 /DNumericVersion=0.0.1 /DChannel=preview
;        /DSourceDir=... /DOutputDir=... /DOutputName=... windows-installer.iss
;
; One Broadsheet sheet (design: the "Windows installer" canvas, B + C): a
; masthead, where it installs, what it adds to Windows, Install. Updates show
; the same sheet with the last choices folded into one line. Every addition is
; per-user and taken back out on uninstall. Written against the Inno 6.4 API.

#if Channel == "preview"
  ; Never change an AppId: it is how upgrades find the existing installation.
  #define AppIdGuid "{{74ED99B0-ACEA-4860-A2AA-86FD0B170CFB}"
  #define AppName "nus Preview"
  #define ChannelLabel "PREVIEW"
  ; The browser registration's names (StartMenuInternet, RegisteredApplications).
  #define BrowserKey "nus-preview"
#elif Channel == "release"
  #define AppIdGuid "{{E991BB0F-52B1-4A72-ABE7-395558E71B60}"
  #define AppName "nus"
  #define ChannelLabel ""
  #define BrowserKey "nus"
#else
  #error Channel must be preview or release
#endif
#define UrlProgId BrowserKey + ".url"

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
; The sheet is the only page before installing: it carries the Install button.
DisableReadyPage=yes
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
; PrepareToInstall closes nus itself after asking once; Restart Manager stays
; as the fallback for anything else holding a file.
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes
ChangesAssociations=yes
SetupLogging=yes
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}

[Messages]
SetupWindowTitle=Install %1
ConfirmUninstall=Remove %1 from this PC?

[Tasks]
; Shown on the sheet, not on Inno's tasks page; the names are the /TASKS= ones.
Name: "path"; Description: "Add the nus shell command (bin\nus.exe) to PATH"
Name: "browser"; Description: "List {#AppName} as a browser in Default apps"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[InstallDelete]
; Like an in-app update, an upgrade replaces the whole package: no runtime
; file from an older version survives beside the new one.
Type: filesandordirs; Name: "{app}\*"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; The sheet's own type and wordmark, used by Setup only.
Source: "{#SourcePath}..\assets\fonts\IBMPlexMono-Regular.ttf"; Flags: dontcopy
Source: "{#SourcePath}..\assets\fonts\IBMPlexMono-SemiBold.ttf"; Flags: dontcopy
Source: "{#SourcePath}..\assets\installer\wordmark-*.bmp"; Flags: dontcopy

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\nus.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\nus.exe"; Tasks: desktopicon

[Registry]
; A browser Windows can offer in Default apps. Only http(s): nus opens a URL
; argument as a page, but a file argument in the editor.
Root: HKCU; Subkey: "Software\Classes\{#UrlProgId}"; ValueType: string; ValueName: ""; ValueData: "{#AppName} URL"; Flags: uninsdeletekey; Tasks: browser
Root: HKCU; Subkey: "Software\Classes\{#UrlProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\nus.exe,0"; Tasks: browser
Root: HKCU; Subkey: "Software\Classes\{#UrlProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\nus.exe"" ""%1"""; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}"; ValueType: string; ValueName: ""; ValueData: "{#AppName}"; Flags: uninsdeletekey; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\nus.exe,0"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\nus.exe"""; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A terminal and browser in one workspace"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: "{app}\nus.exe,0"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities\StartMenu"; ValueType: string; ValueName: "StartMenuInternet"; ValueData: "{#BrowserKey}"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities\URLAssociations"; ValueType: string; ValueName: "http"; ValueData: "{#UrlProgId}"; Tasks: browser
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities\URLAssociations"; ValueType: string; ValueName: "https"; ValueData: "{#UrlProgId}"; Tasks: browser
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#BrowserKey}"; ValueData: "Software\Clients\StartMenuInternet\{#BrowserKey}\Capabilities"; Flags: uninsdeletevalue; Tasks: browser

[Run]
Filename: "{app}\nus.exe"; Description: "Open {#AppName} now"; Flags: nowait postinstall skipifsilent
; Windows lets only the user choose a default browser; this opens nus's own
; page in Default apps (Windows 11), or Default apps itself.
Filename: "ms-settings:defaultapps?registeredAppUser={#BrowserKey}"; Description: "Choose {#AppName} as your browser in Default apps"; Flags: shellexec nowait postinstall skipifsilent; Tasks: browser

[UninstallDelete]
; The folder may hold files an in-app update added after installation.
; Profiles and settings under %LOCALAPPDATA%\nus\installs are kept unless
; the uninstall dialog is told to remove them.
Type: filesandordirs; Name: "{app}"
Type: files; Name: "{localappdata}\nus\installs\{#Channel}\installed-location"

[Code]
const
  EnvironmentKey = 'Environment';
  FR_PRIVATE = $10;
  // Broadsheet tokens, as TColor ($BBGGRR).
  InkColor = $141414;
  DimColor = $606A6F;
  HairColor = $E3E3E3;
  SignalColor = $2E10C8;

var
  Updating, Reinstalling: Boolean;
  PreviousVersion: String;
  UiFont, StrongFont: String;
  SheetPage: TWizardPage;
  SheetShown: Boolean;
  ChoicesPanel, SummaryPanel: TPanel;
  PathCheck, BrowserCheck, DesktopCheck: TNewCheckBox;
  SummaryText, NoteText: TNewStaticText;
  RemoveProfile: Boolean;

function AddFontResourceEx(FileName: String; Flags: Cardinal; Reserved: Cardinal): Integer;
  external 'AddFontResourceExW@gdi32.dll stdcall';

function Sep: String;
begin
  Result := ' ' + #$00B7 + ' ';
end;

function UninstallKey: String;
var
  Guid: String;
begin
  // AppIdGuid carries Inno's doubled brace; the registry key has one.
  Guid := '{#AppIdGuid}';
  Result := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\' + Copy(Guid, 2, Length(Guid) - 1) + '_is1';
end;

{ PATH ------------------------------------------------------------------- }

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

{ Taken back out when an update unticks them; [Registry] only ever adds. }
procedure RemoveBrowser;
begin
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\{#UrlProgId}');
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Clients\StartMenuInternet\{#BrowserKey}');
  RegDeleteValue(HKCU, 'Software\RegisteredApplications', '{#BrowserKey}');
end;

{ nus running from this folder ------------------------------------------- }

function WqlString(const S: String): String;
begin
  Result := S;
  StringChangeEx(Result, '\', '\\', True);
  StringChangeEx(Result, '''', '\''', True);
end;

// Every process whose image lives in the app folder: the app, its browser
// processes and the holders that keep its shells. AppOnly: just nus.exe.
function AppProcesses(AppOnly: Boolean): Variant;
var
  Wmi: Variant;
  Query: String;
begin
  Query := 'SELECT ProcessId FROM Win32_Process WHERE ExecutablePath LIKE ''' +
    WqlString(ExpandConstant('{app}') + '\') + '%''';
  if AppOnly then Query := Query + ' AND Name = ''nus.exe''';
  Wmi := CreateOleObject('WbemScripting.SWbemLocator');
  Wmi := Wmi.ConnectServer('.', 'root\CIMV2');
  Result := Wmi.ExecQuery(Query);
end;

function CountAppProcesses(AppOnly: Boolean): Integer;
var
  Found: Variant;
begin
  try
    Found := AppProcesses(AppOnly);
    Result := Found.Count;
  except
    // No WMI: Restart Manager still finds and closes what holds a file.
    Result := 0;
  end;
end;

procedure WaitForExit(AppOnly: Boolean; Milliseconds: Integer);
var
  Waited: Integer;
begin
  Waited := 0;
  while (CountAppProcesses(AppOnly) > 0) and (Waited < Milliseconds) do begin
    Sleep(250);
    Waited := Waited + 250;
  end;
end;

{ Ask nus's windows to close, as the close button would, then stop whatever
  is left: its held shells run from this folder too. }
procedure CloseApp;
var
  Found, Item: Variant;
  I, Count, Pid, Code: Integer;
  Pids: String;
begin
  try
    Found := AppProcesses(True);
    Count := Found.Count;
    Pids := '';
    for I := 0 to Count - 1 do begin
      Item := Found.ItemIndex(I);
      Pid := Item.ProcessId;
      Pids := Pids + ' /PID ' + IntToStr(Pid);
    end;
    if Pids <> '' then begin
      Exec(ExpandConstant('{sys}\taskkill.exe'), Trim(Pids), '', SW_HIDE, ewWaitUntilTerminated, Code);
      WaitForExit(True, 8000);
    end;
    Found := AppProcesses(False);
    Count := Found.Count;
    for I := 0 to Count - 1 do begin
      Item := Found.ItemIndex(I);
      Item.Terminate(0);
    end;
    if Count > 0 then WaitForExit(False, 5000);
  except
    Log('Could not close nus: ' + GetExceptionMessage);
  end;
end;

{ The sheet ------------------------------------------------------------- }

procedure LoadFonts;
begin
  UiFont := 'Consolas';
  StrongFont := 'Consolas';
  try
    ExtractTemporaryFile('IBMPlexMono-Regular.ttf');
    ExtractTemporaryFile('IBMPlexMono-SemiBold.ttf');
    // Private to Setup: nothing is installed into Windows' fonts.
    if (AddFontResourceEx(ExpandConstant('{tmp}\IBMPlexMono-Regular.ttf'), FR_PRIVATE, 0) > 0) and
       (AddFontResourceEx(ExpandConstant('{tmp}\IBMPlexMono-SemiBold.ttf'), FR_PRIVATE, 0) > 0) then begin
      UiFont := 'IBM Plex Mono';
      StrongFont := 'IBM Plex Mono SemiBold';
    end;
  except
    Log('Using Consolas: ' + GetExceptionMessage);
  end;
end;

procedure SetFont(Font: TFont; Size: Integer; Strong: Boolean; Color: TColor);
begin
  if Strong then Font.Name := StrongFont else Font.Name := UiFont;
  Font.Size := Size;
  Font.Color := Color;
  if Strong and (StrongFont = UiFont) then Font.Style := [fsBold] else Font.Style := [];
end;

function NewText(Parent: TWinControl; X, Y: Integer; Caption: String; Size: Integer; Strong: Boolean; Color: TColor): TNewStaticText;
begin
  Result := TNewStaticText.Create(WizardForm);
  Result.Parent := Parent;
  SetFont(Result.Font, Size, Strong, Color);
  Result.AutoSize := True;
  Result.Left := X;
  Result.Top := Y;
  Result.Caption := Caption;
end;

{ A filled panel: rules, the signal band, and plain white containers. }
function NewBlock(Parent: TWinControl; X, Y, W, H: Integer; Color: TColor): TPanel;
begin
  Result := TPanel.Create(WizardForm);
  Result.Parent := Parent;
  Result.BevelOuter := bvNone;
  Result.ParentBackground := False;
  Result.Color := Color;
  Result.SetBounds(X, Y, W, H);
  // The modern wizard grows when it is shown: full-width pieces grow with it.
  if X + W >= Parent.ClientWidth then Result.Anchors := [akLeft, akTop, akRight];
end;

function KeyWidth: Integer;
begin
  Result := ScaleX(112);
end;

{ A key / value row; returns the next row's top. }
function Row(Parent: TWinControl; Y: Integer; Key, Value: String): Integer;
begin
  NewText(Parent, 0, Y + ScaleY(10), Key, 8, False, DimColor);
  NewText(Parent, KeyWidth, Y + ScaleY(8), Value, 9, False, InkColor);
  Result := Y + ScaleY(32);
end;

function CheckRow(Parent: TWinControl; Y: Integer; Key, Caption, Note: String; var Box: TNewCheckBox): Integer;
begin
  NewText(Parent, 0, Y + ScaleY(10), Key, 8, False, DimColor);
  Box := TNewCheckBox.Create(WizardForm);
  Box.Parent := Parent;
  SetFont(Box.Font, 9, False, InkColor);
  Box.Color := clWhite;
  Box.Caption := Caption;
  Box.SetBounds(KeyWidth, Y + ScaleY(7), Parent.Width - KeyWidth, ScaleY(20));
  Box.Anchors := [akLeft, akTop, akRight];
  Result := Y + ScaleY(32);
  if Note <> '' then begin
    NewText(Parent, KeyWidth + ScaleX(18), Y + ScaleY(27), Note, 8, False, DimColor);
    Result := Y + ScaleY(48);
  end;
end;

function WordmarkFile: String;
var
  Scale: Integer;
  Name: String;
begin
  Scale := ScaleY(100);
  if Scale >= 188 then Name := '200'
  else if Scale >= 163 then Name := '175'
  else if Scale >= 138 then Name := '150'
  else if Scale >= 113 then Name := '125'
  else Name := '100';
  Name := 'wordmark-' + Name + '.bmp';
  ExtractTemporaryFile(Name);
  Result := ExpandConstant('{tmp}\' + Name);
end;

{ Signal band, wordmark, channel and version, over every page. }
procedure BuildMasthead;
var
  Head, Block: TPanel;
  Mark: TBitmapImage;
  Channel, Version: TNewStaticText;
  W, H, X: Integer;
begin
  W := WizardForm.ClientWidth;
  H := ScaleY(60);
  Head := NewBlock(WizardForm, 0, 0, W, H, clWhite);
  Head.Anchors := [akLeft, akTop, akRight];
  Block := NewBlock(Head, 0, 0, W, ScaleY(6), SignalColor);
  Block.Anchors := [akLeft, akTop, akRight];
  Mark := TBitmapImage.Create(WizardForm);
  Mark.Parent := Head;
  Mark.AutoSize := True;
  Mark.Bitmap.LoadFromFile(WordmarkFile);
  Mark.Left := ScaleX(32);
  Mark.Top := ScaleY(18);
  X := Mark.Left + Mark.Width + ScaleX(10);
  if '{#ChannelLabel}' <> '' then begin
    Channel := NewText(Head, X, 0, '{#ChannelLabel}', 8, True, InkColor);
    Channel.Top := Mark.Top + Mark.Height - Channel.Height;
  end;
  if Reinstalling then
    Version := NewText(Head, 0, 0, 'REINSTALL', 8, True, InkColor)
  else if Updating then
    Version := NewText(Head, 0, 0, 'UPDATE', 8, True, InkColor)
  else
    Version := NewText(Head, 0, 0, '{#Version}', 8, False, DimColor);
  Version.Top := Mark.Top + Mark.Height - Version.Height;
  Version.Left := W - ScaleX(32) - Version.Width;
  Version.Anchors := [akTop, akRight];
  Block := NewBlock(Head, 0, H - ScaleY(2), W, ScaleY(2), InkColor);
  Block.Anchors := [akLeft, akTop, akRight];

  WizardForm.OuterNotebook.SetBounds(WizardForm.OuterNotebook.Left, WizardForm.OuterNotebook.Top + H,
    WizardForm.OuterNotebook.Width, WizardForm.OuterNotebook.Height - H);
end;

{ Paper everywhere, no stock header, an ink rule over the buttons. }
procedure RestyleWizard;
var
  Rule: TPanel;
begin
  WizardForm.Color := clWhite;
  WizardForm.InnerPage.Color := clWhite;
  WizardForm.FinishedPage.Color := clWhite;
  WizardForm.PreparingPage.Color := clWhite;
  WizardForm.InstallingPage.Color := clWhite;
  WizardForm.MainPanel.Visible := False;
  WizardForm.Bevel1.Visible := False;
  WizardForm.InnerNotebook.SetBounds(ScaleX(32), ScaleY(12),
    WizardForm.OuterNotebook.ClientWidth - ScaleX(64), WizardForm.OuterNotebook.ClientHeight - ScaleY(12));
  Rule := NewBlock(WizardForm, 0, WizardForm.Bevel.Top, WizardForm.ClientWidth, ScaleY(2), InkColor);
  Rule.Anchors := [akLeft, akRight, akBottom];
  WizardForm.Bevel.Visible := False;
  SetFont(WizardForm.StatusLabel.Font, 9, False, InkColor);
  SetFont(WizardForm.FilenameLabel.Font, 8, False, DimColor);
end;

procedure ShowChoices(Sender: TObject; const Link: String; LinkType: TSysLinkType);
begin
  SummaryPanel.Visible := False;
  ChoicesPanel.Visible := True;
  WizardForm.ActiveControl := PathCheck;
end;

procedure BuildSheet;
var
  S: TNewNotebookPage;
  W, Y, ChoicesTop: Integer;
  Change: TNewLinkLabel;
begin
  SheetPage := CreateCustomPage(wpSelectTasks, '', '');
  S := SheetPage.Surface;
  S.Color := clWhite;
  W := WizardForm.InnerNotebook.ClientWidth;
  Y := 0;
  if Updating then begin
    if Reinstalling then
      Y := Row(S, Y, 'VERSION', '{#Version}, again')
    else
      Y := Row(S, Y, 'VERSION', PreviousVersion + '  ' + #$2192 + '  {#Version}');
    NewBlock(S, 0, Y, W, ScaleY(1), HairColor);
    Y := Row(S, Y, 'KEEPS', 'your profile and settings');
  end else begin
    Y := Row(S, Y, 'INSTALLS TO', '%LOCALAPPDATA%\Programs\nus\{#Channel}');
    NewBlock(S, 0, Y, W, ScaleY(1), HairColor);
    Y := Row(S, Y, 'FOR', 'you only' + Sep + 'no admin' + Sep + 'no restart');
  end;
  NewBlock(S, 0, Y, W, ScaleY(2), InkColor);
  ChoicesTop := Y + ScaleY(2);

  ChoicesPanel := NewBlock(S, 0, ChoicesTop, W, ScaleY(150), clWhite);
  Y := CheckRow(ChoicesPanel, 0, 'SHELL', 'nus command on PATH', 'bin\nus.exe' + Sep + 'for terminals you open next', PathCheck);
  NewBlock(ChoicesPanel, 0, Y, W, ScaleY(1), HairColor);
  Y := CheckRow(ChoicesPanel, Y, 'WEB', 'List nus as a browser in Default apps', '', BrowserCheck);
  NewBlock(ChoicesPanel, 0, Y, W, ScaleY(1), HairColor);
  CheckRow(ChoicesPanel, Y, 'DESKTOP', 'Desktop shortcut', '', DesktopCheck);

  if Updating then begin
    // An update is one click: last time's choices, folded into a line.
    ChoicesPanel.Visible := False;
    SummaryPanel := NewBlock(S, 0, ChoicesTop, W, ScaleY(32), clWhite);
    NewText(SummaryPanel, 0, ScaleY(10), 'AS BEFORE', 8, False, DimColor);
    SummaryText := NewText(SummaryPanel, KeyWidth, ScaleY(8), '', 9, False, InkColor);
    Change := TNewLinkLabel.Create(WizardForm);
    Change.Parent := SummaryPanel;
    SetFont(Change.Font, 9, False, InkColor);
    Change.Caption := '<a id="change">change</a>';
    Change.Top := ScaleY(8);
    Change.Left := W - Change.Width;
    Change.Anchors := [akTop, akRight];
    Change.OnLinkClick := @ShowChoices;
  end;

  NoteText := NewText(S, 0, 0, '', 8, False, DimColor);
end;

function DescribeChoices: String;
begin
  Result := '';
  if PathCheck.Checked then Result := 'nus on PATH';
  if BrowserCheck.Checked then begin
    if Result <> '' then Result := Result + Sep;
    Result := Result + 'browser';
  end;
  if DesktopCheck.Checked then begin
    if Result <> '' then Result := Result + Sep;
    Result := Result + 'desktop shortcut';
  end;
  if Result = '' then Result := 'nothing added to Windows';
end;

procedure ShowSheet;
begin
  if not SheetShown then begin
    // [Tasks] holds the defaults, the last install's choices and any /TASKS=.
    PathCheck.Checked := WizardIsTaskSelected('path');
    BrowserCheck.Checked := WizardIsTaskSelected('browser');
    DesktopCheck.Checked := WizardIsTaskSelected('desktopicon');
    if Updating then SummaryText.Caption := DescribeChoices;
    SheetShown := True;
  end;
  if Reinstalling then
    WizardForm.NextButton.Caption := 'Reinstall'
  else if Updating then
    WizardForm.NextButton.Caption := 'Update'
  else
    WizardForm.NextButton.Caption := SetupMessage(msgButtonInstall);
  if not Updating then
    NoteText.Caption := 'Uninstall takes all of this back out.'
  else if CountAppProcesses(True) > 0 then
    NoteText.Caption := 'nus is open: ' + WizardForm.NextButton.Caption + ' closes it. Reopen it from the last step.'
  else
    NoteText.Caption := '';
  NoteText.Top := WizardForm.InnerNotebook.ClientHeight - NoteText.Height;
end;

procedure SaveSheet;
var
  Tasks: String;
begin
  if PathCheck.Checked then Tasks := 'path' else Tasks := '!path';
  if BrowserCheck.Checked then Tasks := Tasks + ',browser' else Tasks := Tasks + ',!browser';
  if DesktopCheck.Checked then Tasks := Tasks + ',desktopicon' else Tasks := Tasks + ',!desktopicon';
  WizardSelectTasks(Tasks);
end;

procedure ShowFinished;
var
  Heading, Body: TNewStaticText;
  X, W: Integer;
  Lines: String;
begin
  WizardForm.WizardBitmapImage2.Visible := False;
  X := ScaleX(32);
  W := WizardForm.OuterNotebook.ClientWidth - 2 * X;
  Heading := WizardForm.FinishedHeadingLabel;
  SetFont(Heading.Font, 14, True, InkColor);
  Heading.SetBounds(X, ScaleY(22), W, Heading.Height);
  if Reinstalling then Heading.Caption := 'Reinstalled.'
  else if Updating then Heading.Caption := 'Updated.'
  else Heading.Caption := 'Installed.';
  WizardForm.AdjustLabelHeight(Heading);

  Lines := 'Start menu' + Sep + '{#AppName}';
  if WizardIsTaskSelected('path') then Lines := Lines + #13#10 + 'The nus command works in terminals you open from now on.';
  Body := WizardForm.FinishedLabel;
  SetFont(Body.Font, 9, False, InkColor);
  Body.SetBounds(X, Heading.Top + Heading.Height + ScaleY(16), W, Body.Height);
  Body.Caption := Lines;
  WizardForm.AdjustLabelHeight(Body);

  SetFont(WizardForm.RunList.Font, 9, False, InkColor);
  WizardForm.RunList.Color := clWhite;
  WizardForm.RunList.SetBounds(X, Body.Top + Body.Height + ScaleY(18), W, WizardForm.RunList.Height);
end;

{ Events ---------------------------------------------------------------- }

function InitializeSetup: Boolean;
begin
  Updating := RegQueryStringValue(HKCU, UninstallKey, 'DisplayVersion', PreviousVersion);
  Reinstalling := Updating and (PreviousVersion = '{#Version}');
  Result := True;
end;

procedure InitializeWizard;
begin
  LoadFonts;
  if Reinstalling then WizardForm.Caption := 'Reinstall {#AppName}'
  else if Updating then WizardForm.Caption := 'Update {#AppName}';
  BuildMasthead;
  RestyleWizard;
  BuildSheet;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  // The sheet carries the tasks.
  Result := PageID = wpSelectTasks;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = SheetPage.ID then ShowSheet
  else if CurPageID = wpFinished then ShowFinished;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  // Silent installs "click" through too; there /TASKS= decides.
  if (CurPageID = SheetPage.ID) and not WizardSilent then SaveSheet;
  Result := True;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if CountAppProcesses(False) = 0 then exit;
  if not WizardSilent then
    if TaskDialogMsgBox('{#AppName} is open', 'Setup needs to close it to update. Shells running in nus will end.',
         mbConfirmation, MB_OKCANCEL, ['Close nus and update'#10'Reopen it from the last step.'], 0) <> IDOK then begin
      Result := '{#AppName} is still open. Close it, then run Setup again.';
      exit;
    end;
  CloseApp;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Channel: String;
begin
  if CurStep <> ssPostInstall then exit;
  if WizardIsTaskSelected('path') then AddToPath else RemoveFromPath;
  if not WizardIsTaskSelected('browser') then RemoveBrowser;
  if not WizardIsTaskSelected('desktopicon') then DeleteFile(ExpandConstant('{autodesktop}\{#AppName}.lnk'));
  // nus reads this to recognize its installed copy (install.rs installed_here).
  Channel := ExpandConstant('{localappdata}\nus\installs\{#Channel}');
  ForceDirectories(Channel);
  SaveStringToFile(Channel + '\installed-location', ExpandConstant('{app}'), False);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then begin
    // Silent uninstalls keep the profile, as they always have.
    if not UninstallSilent then
      RemoveProfile := TaskDialogMsgBox('Keep your settings?',
        'If nus is open, it closes now. Shells running in it will end.',
        mbConfirmation, MB_YESNO, ['Keep my settings'#10'Profiles and settings stay for a reinstall.',
          'Remove them too'#10'Deletes {#AppName}''s profile, settings and sign-ins on this PC.'], 0) = IDNO;
    CloseApp;
  end else if CurUninstallStep = usPostUninstall then begin
    RemoveFromPath;
    if RemoveProfile then
      DelTree(ExpandConstant('{localappdata}\nus\installs\{#Channel}'), True, True, True);
  end;
end;
