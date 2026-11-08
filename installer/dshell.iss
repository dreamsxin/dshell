#define AppName "dshell"
#define AppVersion "0.1.50"
#define AppExeName "dshell.exe"

[Setup]
AppId={{4891BF02-0AFD-4EFB-8E22-CC66D969AB93}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=dshell
AppCopyright=Copyright (C) 2026 dshell contributors
DefaultDirName={localappdata}\Programs\dshell
DefaultGroupName=dshell
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}
SetupIconFile=..\assets\dshell.ico
WizardSmallImageFile=..\assets\dshell-installer-wizard.bmp
LicenseFile=..\LICENSE
OutputDir=..\dist\installer
OutputBaseFilename=dshell-setup-v{#AppVersion}-x64
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=lowest
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
VersionInfoVersion=0.1.50.0
VersionInfoProductVersion={#AppVersion}
VersionInfoProductName=dshell

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\dshell-cli.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\dshell"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\{#AppExeName}"
Name: "{autodesktop}\dshell"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch dshell"; Flags: nowait postinstall skipifsilent unchecked
