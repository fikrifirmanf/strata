; Inno Setup script for the Windows installer. Built by scripts/package-windows.ps1:
;   ISCC.exe /DAppVersion=0.2.0 scripts\strata.iss
; Paths are relative to this file. Installs per user by default (no admin prompt); the
; privileges dialog lets an administrator install for all users instead.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
; Numbers only (no "-beta.1"); Windows file version fields reject anything else.
#ifndef FileVersion
  #define FileVersion "0.0.0"
#endif

[Setup]
AppId={{31984120-54AA-4355-AFFE-63E72DB7B8E7}
AppName=Strata
AppVersion={#AppVersion}
AppVerName=Strata {#AppVersion}
AppPublisher=Fikri Firman Fadilah
AppPublisherURL=https://github.com/fikrifirmanf/strata
AppSupportURL=https://github.com/fikrifirmanf/strata/issues
AppUpdatesURL=https://github.com/fikrifirmanf/strata/releases
VersionInfoVersion={#FileVersion}
DefaultDirName={autopf}\Strata
DefaultGroupName=Strata
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon\strata.ico
UninstallDisplayIcon={app}\strata.exe
UninstallDisplayName=Strata
OutputDir=..\dist
OutputBaseFilename=Strata-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Lets the installer replace strata.exe while an older copy is running.
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\strata.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Strata"; Filename: "{app}\strata.exe"; AppUserModelID: "dev.strata.app"
Name: "{autodesktop}\Strata"; Filename: "{app}\strata.exe"; Tasks: desktopicon; AppUserModelID: "dev.strata.app"

[Run]
Filename: "{app}\strata.exe"; Description: "{cm:LaunchProgram,Strata}"; Flags: nowait postinstall skipifsilent
; In-app updates run the installer with /SILENT; start the new version when it finishes.
Filename: "{app}\strata.exe"; Flags: nowait runasoriginaluser skipifnotsilent

; Connections and settings in %APPDATA%\Strata and passwords in Credential Manager are kept on
; uninstall, so reinstalling or upgrading loses nothing.
