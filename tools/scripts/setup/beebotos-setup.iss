; BeeBotOS Windows Installer Script (Inno Setup)
; ============================================================
; Usage:
;   1. Build the release package: pwsh .\beebotos-dev.ps1 pack all
;   2. Copy dist\beebotos to your installer staging area
;   3. Update #define SourceRoot below to point to that directory
;   4. Compile this script with Inno Setup Compiler (iscmplr.exe)
; ============================================================

#define MyAppName "BeeBotOS"
#define MyAppVersion "1.5.0"
#define MyAppPublisher "BeeBotOS Team"
#define MyAppURL "https://github.com/linc77/beebotos"
; ⚠️ Update this path to your actual staging directory before compiling
#define SourceRoot "C:\Users\you\Desktop\beebotos_installer\beebotos"
#define MyAppExeName "beebotos-gateway.exe"

[Setup]
AppId={{BEE-BOTO-SAPP-0000-000000000001}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName=C:\Program Files\{#MyAppName}
DisableProgramGroupPage=yes
; LicenseFile={#SourceRoot}\LICENSE.txt
; Uncomment the next line if you have an output directory set up
; OutputDir=C:\Users\you\Desktop\beebotos_installer\output
OutputBaseFilename=BeeBotOS-{#MyAppVersion}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkablealone

[Dirs]
Name: "{app}\data"
Name: "{app}\data\run"
Name: "{app}\data\logs"
Name: "{app}\data\skills"
Name: "{app}\data\workspace"

[Files]
; Install all application files recursively
Source: "{#SourceRoot}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}\启动 {#MyAppName}"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" start all"; WorkingDir: "{app}"
Name: "{autoprograms}\{#MyAppName}\停止 {#MyAppName}"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" stop all"; WorkingDir: "{app}"
Name: "{autoprograms}\{#MyAppName}\查看状态"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -NoExit -File ""{app}\beebotos-run.ps1"" status"; WorkingDir: "{app}"
Name: "{autoprograms}\{#MyAppName}\{#MyAppName} Web"; Filename: "http://localhost:8090"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" start all"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" start all"; WorkingDir: "{app}"; Description: "启动 {#MyAppName}"; Flags: postinstall skipifsilent unchecked

[UninstallRun]
; Stop all services before uninstalling
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" stop all"; WorkingDir: "{app}"; Flags: runhidden; RunOnceId: "StopBeeBotOS"

; ============================================================
; 🛡️ PRESERVE USER DATA ON UNINSTALL
; Strategy: Move database files to a safe location BEFORE
; the uninstaller deletes anything, then restore AFTER.
; Using {userappdata} which is outside the install directory.
; ============================================================

[Code]
var
  DBFilesExist: Boolean;

// Return the permanent backup directory in user's AppData (not temp!)
function GetDBBackupDir: String;
begin
  Result := ExpandConstant('{userappdata}') + '\BeeBotOS\DB_Backup';
end;

// Check if any database files exist
function CheckDBFilesExist: Boolean;
var
  DataDir: String;
begin
  DataDir := ExpandConstant('{app}') + '\data';
  Result := FileExists(DataDir + '\beebotos.db') or
            FileExists(DataDir + '\memory_search.db') or
            FileExists(DataDir + '\beebotos.db-wal') or
            FileExists(DataDir + '\memory_search.db-wal') or
            FileExists(DataDir + '\beebotos.db-shm') or
            FileExists(DataDir + '\memory_search.db-shm');
end;

// Move (not copy) database files to backup directory
// Using CopyFile + DeleteFile ensures the files are gone from {app} before uninstaller runs
procedure MoveDBFilesToBackup;
var
  DataDir: String;
  BackupDir: String;
begin
  DataDir := ExpandConstant('{app}') + '\data';
  BackupDir := GetDBBackupDir;

  // Clean up any old backup first
  if DirExists(BackupDir) then
  begin
    DelTree(BackupDir, True, True, True);
  end;

  if not ForceDirectories(BackupDir) then
  begin
    Log('Failed to create DB backup directory: ' + BackupDir);
    Exit;
  end;

  // Move beebotos.db
  if FileExists(DataDir + '\beebotos.db') then
  begin
    if not CopyFile(DataDir + '\beebotos.db', BackupDir + '\beebotos.db', False) then
      Log('Failed to backup beebotos.db')
    else
      DeleteFile(DataDir + '\beebotos.db');
  end;

  // Move memory_search.db
  if FileExists(DataDir + '\memory_search.db') then
  begin
    if not CopyFile(DataDir + '\memory_search.db', BackupDir + '\memory_search.db', False) then
      Log('Failed to backup memory_search.db')
    else
      DeleteFile(DataDir + '\memory_search.db');
  end;

  // Move WAL files
  if FileExists(DataDir + '\beebotos.db-wal') then
  begin
    if not CopyFile(DataDir + '\beebotos.db-wal', BackupDir + '\beebotos.db-wal', False) then
      Log('Failed to backup beebotos.db-wal')
    else
      DeleteFile(DataDir + '\beebotos.db-wal');
  end;

  if FileExists(DataDir + '\memory_search.db-wal') then
  begin
    if not CopyFile(DataDir + '\memory_search.db-wal', BackupDir + '\memory_search.db-wal', False) then
      Log('Failed to backup memory_search.db-wal')
    else
      DeleteFile(DataDir + '\memory_search.db-wal');
  end;

  // Move SHM files
  if FileExists(DataDir + '\beebotos.db-shm') then
  begin
    if not CopyFile(DataDir + '\beebotos.db-shm', BackupDir + '\beebotos.db-shm', False) then
      Log('Failed to backup beebotos.db-shm')
    else
      DeleteFile(DataDir + '\beebotos.db-shm');
  end;

  if FileExists(DataDir + '\memory_search.db-shm') then
  begin
    if not CopyFile(DataDir + '\memory_search.db-shm', BackupDir + '\memory_search.db-shm', False) then
      Log('Failed to backup memory_search.db-shm')
    else
      DeleteFile(DataDir + '\memory_search.db-shm');
  end;

  Log('Database files moved to backup: ' + BackupDir);
end;

// Restore database files from backup
procedure RestoreDBFilesFromBackup;
var
  DataDir: String;
  BackupDir: String;
begin
  DataDir := ExpandConstant('{app}') + '\data';
  BackupDir := GetDBBackupDir;

  if not DirExists(BackupDir) then
  begin
    Log('No DB backup directory found, nothing to restore');
    Exit;
  end;

  // Ensure data directory exists
  if not ForceDirectories(DataDir) then
  begin
    Log('Failed to recreate data directory: ' + DataDir);
    Exit;
  end;

  // Restore beebotos.db
  if FileExists(BackupDir + '\beebotos.db') then
  begin
    CopyFile(BackupDir + '\beebotos.db', DataDir + '\beebotos.db', False);
  end;

  // Restore memory_search.db
  if FileExists(BackupDir + '\memory_search.db') then
  begin
    CopyFile(BackupDir + '\memory_search.db', DataDir + '\memory_search.db', False);
  end;

  // Restore WAL files
  if FileExists(BackupDir + '\beebotos.db-wal') then
  begin
    CopyFile(BackupDir + '\beebotos.db-wal', DataDir + '\beebotos.db-wal', False);
  end;

  if FileExists(BackupDir + '\memory_search.db-wal') then
  begin
    CopyFile(BackupDir + '\memory_search.db-wal', DataDir + '\memory_search.db-wal', False);
  end;

  // Restore SHM files
  if FileExists(BackupDir + '\beebotos.db-shm') then
  begin
    CopyFile(BackupDir + '\beebotos.db-shm', DataDir + '\beebotos.db-shm', False);
  end;

  if FileExists(BackupDir + '\memory_search.db-shm') then
  begin
    CopyFile(BackupDir + '\memory_search.db-shm', DataDir + '\memory_search.db-shm', False);
  end;

  Log('Database files restored from: ' + BackupDir);

  // Clean up backup directory
  DelTree(BackupDir, True, True, True);
end;

// Called BEFORE uninstallation begins
function InitializeUninstall(): Boolean;
begin
  Result := true;
  DBFilesExist := CheckDBFilesExist;
  
  if DBFilesExist then
  begin
    Log('Database files detected, moving to safe backup location');
    MoveDBFilesToBackup;
  end
  else
  begin
    Log('No database files found, nothing to backup');
  end;
end;

// Called AFTER uninstallation completes
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    if DBFilesExist then
    begin
      Log('Restoring database files after uninstall');
      RestoreDBFilesFromBackup;
    end;
  end;
end;
