#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{3E5A9C21-7D4B-4F6E-9A1C-5B8D2E0F7A63}
AppName=Stagehand
AppVersion={#AppVersion}
AppPublisher=Cheter
DefaultDirName={autopf}\Stagehand
DefaultGroupName=Stagehand
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Stagehand-Setup-{#AppVersion}
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\stagehand.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\stage\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Stagehand"; Filename: "{app}\stagehand.exe"
Name: "{autodesktop}\Stagehand"; Filename: "{app}\stagehand.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\stagehand.exe"; Description: "{cm:LaunchProgram,Stagehand}"; Flags: nowait postinstall
