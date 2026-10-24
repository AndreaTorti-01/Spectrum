; Spectrum per-user installer (Inno Setup 6). No admin: everything lives under the user's profile.
; Build:  iscc installer\spectrum.iss [/DAppVersion=1.2.3] [/DExePath=C:\path\to\Spectrum.exe]
; Run `cargo build --release` first: it also writes target\spectrum.ico, the wizard icon.

#ifndef AppVersion
  #define AppVersion "0.1.6"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\Spectrum.exe"
#endif

[Setup]
AppId={{9670FE50-F9DF-44C5-862D-DE6C0E81AD1B}
AppName=Spectrum
AppVersion={#AppVersion}
AppPublisher=Andrea Torti
VersionInfoDescription=Spectrum setup
DefaultDirName={localappdata}\Programs\Spectrum
DisableDirPage=yes
DisableProgramGroupPage=yes
; Per-user only. Leaving PrivilegesRequiredOverridesAllowed unset keeps it that way: no admin mode.
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
SetupIconFile=..\target\spectrum.ico
UninstallDisplayIcon={app}\Spectrum.exe
UninstallDisplayName=Spectrum
OutputDir=..\target\installer
OutputBaseFilename=SpectrumSetup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; No AppMutex: an update closes the running copy itself (PrepareToInstall below) instead of asking the user to.
; Restart Manager stays on as a fallback for anything still holding the exe.
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "startup"; Description: "Start with Windows"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Spectrum"; Filename: "{app}\Spectrum.exe"
; The in-app "Start with Windows" checkbox creates and removes this same file: Spectrum.lnk with --tray.
; On an update it keeps whatever state the user left it in (WantStartupShortcut).
Name: "{userstartup}\Spectrum"; Filename: "{app}\Spectrum.exe"; Parameters: "--tray"; Check: WantStartupShortcut

[Run]
Filename: "{app}\Spectrum.exe"; Description: "{cm:LaunchProgram,Spectrum}"; Flags: nowait postinstall skipifsilent; Check: not WasRunning
; An update relaunches the copy it closed, in the tray, also when run with /SILENT or /VERYSILENT.
Filename: "{app}\Spectrum.exe"; Parameters: "--tray"; Flags: nowait; Check: WasRunning

[UninstallRun]
Filename: "{app}\Spectrum.exe"; Parameters: "--quit"; RunOnceId: "QuitSpectrum"; Flags: runhidden waituntilterminated
Filename: "{app}\Spectrum.exe"; Parameters: "--cleanup"; RunOnceId: "CleanupSpectrum"; Flags: runhidden waituntilterminated

[Code]
const
  // Must match MUTEX_NAME in src/main.rs.
  InstanceMutex = 'Local\Spectrum.SingleInstance';

var
  Upgrading: Boolean;
  HadStartupShortcut: Boolean;
  SpectrumWasRunning: Boolean;

function StartupShortcut(): String;
begin
  Result := ExpandConstant('{userstartup}\Spectrum.lnk');
end;

function InitializeSetup(): Boolean;
begin
  Upgrading := FileExists(ExpandConstant('{localappdata}\Programs\Spectrum\Spectrum.exe'));
  HadStartupShortcut := FileExists(StartupShortcut());
  Result := True;
end;

// An update keeps the user's current "Start with Windows" choice, so the tasks page only shows on a fresh install.
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := Upgrading and (PageID = wpSelectTasks);
end;

function WantStartupShortcut(): Boolean;
begin
  if Upgrading then
    Result := HadStartupShortcut
  else
    Result := WizardIsTaskSelected('startup');
end;

function WasRunning(): Boolean;
begin
  Result := SpectrumWasRunning;
end;

// Closes a running copy before its exe is overwritten. The new exe's --quit asks the tray window to exit and
// waits for the process; the mutex loop covers an old copy that is slow to go.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code, Waited: Integer;
begin
  Result := '';
  SpectrumWasRunning := CheckForMutexes(InstanceMutex);
  if not SpectrumWasRunning then
    Exit;
  ExtractTemporaryFile('Spectrum.exe');
  Exec(ExpandConstant('{tmp}\Spectrum.exe'), '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Waited := 0;
  while CheckForMutexes(InstanceMutex) and (Waited < 10000) do
  begin
    Sleep(100);
    Waited := Waited + 100;
  end;
  if CheckForMutexes(InstanceMutex) then
    Result := 'Spectrum is still running. Quit it from the tray icon, then click Back and Next.';
end;
