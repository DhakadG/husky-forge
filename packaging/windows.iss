; Inno Setup script — built by release.yml on windows-latest (ISCC is preinstalled).
#define AppVersion GetEnv("FORGE_VERSION")
[Setup]
AppId={{7E1D6B2A-5C1F-4E1B-9F1A-3C2B1A0F9E8D}
AppName=Husky Forge
AppVersion={#AppVersion}
AppPublisher=DhakadG
AppPublisherURL=https://github.com/DhakadG/husky-forge
DefaultDirName={autopf}\Husky Forge
DefaultGroupName=Husky Forge
UninstallDisplayIcon={app}\husky-forge.exe
OutputDir=..\dist
OutputBaseFilename=husky-forge-{#AppVersion}-windows-x64-setup
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
PrivilegesRequiredOverridesAllowed=dialog

[Files]
Source: "..\dist\win\husky-forge.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\win\forge.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Husky Forge"; Filename: "{app}\husky-forge.exe"
Name: "{autodesktop}\Husky Forge"; Filename: "{app}\husky-forge.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "contextmenu"; Description: "Add ""Forge with Husky"" to the Explorer right-click menu"
Name: "path"; Description: "Add the forge command-line tool to PATH"; Flags: unchecked

[Registry]
; Right-click on folders and image files → open them in Husky Forge.
Root: HKA; Subkey: "Software\Classes\Directory\shell\HuskyForge"; ValueType: string; ValueName: ""; ValueData: "Forge with Husky"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\HuskyForge"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\husky-forge.exe"""; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\HuskyForge\command"; ValueType: string; ValueName: ""; ValueData: """{app}\husky-forge.exe"" ""%1"""; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\*\shell\HuskyForge"; ValueType: string; ValueName: ""; ValueData: "Forge with Husky"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\*\shell\HuskyForge"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\husky-forge.exe"""; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\*\shell\HuskyForge\command"; ValueType: string; ValueName: ""; ValueData: """{app}\husky-forge.exe"" ""%1"""; Tasks: contextmenu
Root: HKA; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: path; Check: not PathHas(ExpandConstant('{app}'))

[Run]
Filename: "{app}\husky-forge.exe"; Description: "Launch Husky Forge"; Flags: nowait postinstall skipifsilent

[Code]
function PathHas(Dir: string): Boolean;
var P: string;
begin
  if not RegQueryStringValue(HKA, 'Environment', 'Path', P) then P := '';
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(P) + ';') > 0;
end;
