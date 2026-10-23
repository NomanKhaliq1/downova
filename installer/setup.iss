; Inno Setup script. Do not run it directly: scripts\build-installer.ps1 writes dist\brand.iss from brand\brand.toml
; (name, product id, company, version, GUID) and then compiles this file. Nothing about the product is hard-coded here.
#include "..\dist\brand.iss"

[Setup]
AppId={{{#AppGuid}}
AppName={#BrandName}
AppVersion={#Version}
AppVerName={#BrandName} {#Version}
AppPublisher={#Company}
AppPublisherURL={#Homepage}
AppSupportURL={#Homepage}
VersionInfoVersion={#Version}
VersionInfoProductName={#BrandName}
; per-user install: no administrator rights, no UAC prompt
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#BrandName}
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=..\dist
OutputBaseFilename={#ProductId}-setup-{#Version}
SetupIconFile=..\brand\assets\app.ico
UninstallDisplayIcon={app}\dmd.exe
UninstallDisplayName={#BrandName}
WizardStyle=modern
WizardImageFile=..\brand\assets\wizard-large.bmp,..\brand\assets\wizard-large@2x.bmp
WizardSmallImageFile=..\brand\assets\wizard-small.bmp,..\brand\assets\wizard-small@2x.bmp
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; the running app is closed gracefully by our own code below, not by Restart Manager
CloseApplications=no
RestartApplications=no
; Authenticode signing: set SignTool in the compiler (Tools > Configure Sign Tools) and uncomment
; SignTool=signtool
; SignedUninstaller=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start {#BrandName} when Windows starts (runs quietly in the tray)"; GroupDescription: "Options:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Options:"; Flags: unchecked

[Files]
Source: "..\target\release\dmd.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\dmhost.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ui\*"; DestDir: "{app}\ui"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\dist\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#BrandName}"; Filename: "{app}\dmd.exe"
Name: "{autodesktop}\{#BrandName}"; Filename: "{app}\dmd.exe"; Tasks: desktopicon

[Registry]
; same value the app writes itself when "start with Windows" is switched on in its settings
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#ProductId}"; ValueData: """{app}\dmd.exe"" --background"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
; tell Chrome, Edge and Brave where the browser helper is (current user only)
Filename: "{app}\dmhost.exe"; Parameters: "install"; Flags: runhidden; StatusMsg: "Connecting your browsers..."
Filename: "{app}\dmd.exe"; Description: "Start {#BrandName} now"; Flags: nowait postinstall skipifsilent
#if StoreUrl != ""
Filename: "{#StoreUrl}"; Description: "Add the {#BrandName} browser extension"; Flags: shellexec nowait postinstall skipifsilent
#else
Filename: "{app}\extension"; Description: "Open the browser extension folder (Chrome/Edge: Extensions > Developer mode > Load unpacked)"; Flags: shellexec nowait postinstall skipifsilent unchecked
#endif

[UninstallDelete]
Type: files; Name: "{app}\com.{#ProductId}.host.json"
Type: dirifempty; Name: "{app}"

[Code]
// Ask a running copy to save its list and exit (used before upgrading and before removing).
procedure QuitRunningApp(AppDir: String);
var
  Host: String;
  Code: Integer;
begin
  Host := AppDir + '\dmhost.exe';
  if FileExists(Host) then
    Exec(Host, 'quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitRunningApp(ExpandConstant('{app}'));
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Host: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    QuitRunningApp(ExpandConstant('{app}'));
    Host := ExpandConstant('{app}\dmhost.exe');
    if FileExists(Host) then
      Exec(Host, 'uninstall', '', SW_HIDE, ewWaitUntilTerminated, Code);
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    // the download list and settings are the user's: keep them unless asked (a silent uninstall always keeps them)
    if not UninstallSilent then
      if MsgBox('Also delete your download list and settings?' + #13#10 + #13#10 +
                'Files you downloaded are never deleted.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(ExpandConstant('{userappdata}\{#ProductId}'), True, True, True);
  end;
end;
