; Spectrum per-user installer (Inno Setup 6). No admin: everything lives under the user's profile.
; Build:  iscc installer\spectrum.iss [/DAppVersion=1.2.3] [/DExePath=C:\path\to\Spectrum.exe]
; Run `cargo build --release` first: it also writes target\spectrum.ico, the wizard icon.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\Spectrum.exe"
#endif

[Setup]
AppId={{9670FE50-F9DF-44C5-862D-DE6C0E81AD1B}
AppName=Spectrum
AppVersion={#AppVersion}
AppPublisher=Spectrum contributors
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
; Must match MUTEX_NAME in src/main.rs. Setup and the uninstaller ask the user to close a running copy.
AppMutex=Local\Spectrum.SingleInstance
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "startup"; Description: "Start with Windows"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Spectrum"; Filename: "{app}\Spectrum.exe"
; The in-app "Start with Windows" checkbox creates and removes this same file: Spectrum.lnk with --tray.
Name: "{userstartup}\Spectrum"; Filename: "{app}\Spectrum.exe"; Parameters: "--tray"; Tasks: startup

[Run]
Filename: "{app}\Spectrum.exe"; Description: "{cm:LaunchProgram,Spectrum}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\Spectrum.exe"; Parameters: "--quit"; RunOnceId: "QuitSpectrum"; Flags: runhidden waituntilterminated
Filename: "{app}\Spectrum.exe"; Parameters: "--cleanup"; RunOnceId: "CleanupSpectrum"; Flags: runhidden waituntilterminated
