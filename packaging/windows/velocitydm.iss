; Inno Setup script for VelocityDM (Windows 7+ with the win7 payload,
; Windows 10+ with the standard payload).
; Build with ISCC (Inno Setup 6+) on Windows or via wine:
;   iscc packaging/windows/velocitydm.iss
;
; Expects the following layout next to this file (produce with
; scripts/package-windows.sh):
;   payload/velocitydm.exe      (GUI build)
;   payload/velocitydm-nmh.exe  (native messaging host)

#define AppName "VelocityDM"
#define AppVersion "1.0.0"
#define AppPublisher "VelocityDM contributors"
#define AppExeName "velocitydm.exe"

[Setup]
AppId={{7C5A6B1D-92E4-4B2A-9E1F-VELOCITYDM01}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}
OutputBaseFilename=velocitydm-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
MinVersion=6.1
WizardStyle=modern

[Files]
Source: "payload\velocitydm.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\velocitydm-nmh.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\nmh\com.velocitydm.nmh.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\nmh\com.velocitydm.nmh.firefox.json"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{localappdata}\{#AppName}\downloads"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional icons:"
Name: "registernmh"; Description: "Register native messaging host for Chrome/Edge/Firefox"; GroupDescription: "Browser integration:"; Flags: checkedonce

[Registry]
; native messaging host manifests (per-user)
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\com.velocitydm.nmh"; ValueType: string; ValueData: "{app}\com.velocitydm.nmh.json"; Flags: uninsdeletekey; Tasks: registernmh
Root: HKCU; Subkey: "Software\Chromium\NativeMessagingHosts\com.velocitydm.nmh"; ValueType: string; ValueData: "{app}\com.velocitydm.nmh.json"; Flags: uninsdeletekey; Tasks: registernmh
Root: HKCU; Subkey: "Software\Mozilla\NativeMessagingHosts\com.velocitydm.nmh"; ValueType: string; ValueData: "{app}\com.velocitydm.nmh.firefox.json"; Flags: uninsdeletekey; Tasks: registernmh

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
var
  ChromePath, EdgePath: string;
begin
  if CurStep = ssPostInstall then
  begin
    // patch the manifest with the real binary path + placeholder origins
    SaveStringToFile(ExpandConstant('{app}\com.velocitydm.nmh.json'),
      '{'#13#10 +
      '  "name": "com.velocitydm.nmh",'#13#10 +
      '  "description": "VelocityDM native messaging host",'#13#10 +
      '  "path": "' + ExpandConstant('{app}\velocitydm-nmh.exe') + '",'#13#10 +
      '  "type": "stdio",'#13#10 +
      '  "allowed_origins": ["chrome-extension://REPLACE_WITH_EXTENSION_ID/"]'#13#10 +
      '}', False);
    SaveStringToFile(ExpandConstant('{app}\com.velocitydm.nmh.firefox.json'),
      '{'#13#10 +
      '  "name": "com.velocitydm.nmh",'#13#10 +
      '  "description": "VelocityDM native messaging host",'#13#10 +
      '  "path": "' + ExpandConstant('{app}\velocitydm-nmh.exe') + '",'#13#10 +
      '  "type": "stdio",'#13#10 +
      '  "allowed_extensions": ["velocitydm@velocitydm.example"]'#13#10 +
      '}', False);
  end;
end;
