<#
.SYNOPSIS
Installs Statup as a Windows service, or updates it when it is installed.

.DESCRIPTION
Run in PowerShell as an administrator:

    irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1 | iex

or, with parameters:

    & ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Port 8080

Every question has its parameter, and -Unattended takes the default of the
others. Run again, it updates Statup and keeps its settings.

.PARAMETER InstallDir
Program folder. Default: C:\Program Files\Statup.

.PARAMETER DataDir
Data folder. Default: C:\ProgramData\Statup.

.PARAMETER BackupDir
Backup folder, a share such as \\nas\statup included. Default: the backups
folder of the data folder.

.PARAMETER Port
Port. Default: 3000.

.PARAMETER Listen
all for every address, local for this machine only, behind a reverse proxy,
or an address. Default: all.

.PARAMETER ServiceAccount
Virtual (the account NT SERVICE\Statup), LocalSystem, a group managed service
account such as DOMAIN\statup$, or an account given with -Credential.
Default: Virtual.

.PARAMETER Credential
The password of an account given in -ServiceAccount. It is handed to Windows
and never written to a file.

.PARAMETER Firewall
Open the port in Windows Firewall. Asked by default.

.PARAMETER NoFirewall
Leave Windows Firewall as it is.

.PARAMETER FirewallProfile
Domain, Private or Public, one or more. Default: the profiles of the networks
this machine is on.

.PARAMETER Version
Version to install. Default: the latest.

.PARAMETER Archive
Install from an archive already downloaded.

.PARAMETER Mirror
Download from a copy of the release files, served as <url>/v<version>/<file>.
Needs -Version.

.PARAMETER Unattended
Take the default of every question not answered, and confirm -Uninstall.

.PARAMETER Uninstall
Remove Statup; keep its data and settings.
#>
[CmdletBinding()]
param(
    [string] $InstallDir,
    [string] $DataDir,
    [string] $BackupDir,
    [string] $Port,
    [string] $Listen,
    [string] $ServiceAccount,
    [pscredential] $Credential,
    [switch] $Firewall,
    [switch] $NoFirewall,
    [string[]] $FirewallProfile,
    [string] $Version,
    [string] $Archive,
    [string] $Mirror,
    [switch] $Unattended,
    [switch] $Uninstall
)

function Get-StatupSettingsPath {
    [IO.Path]::Combine($env:ProgramData, 'Statup', 'statup.env')
}

function Test-StatupConsole {
    if ([Console]::IsInputRedirected) {
        throw 'no console to answer the questions: pass -Unattended, with the parameters you need'
    }
}

# The value given as a parameter, else the answer, else the default.
function Read-StatupChoice {
    param([hashtable] $Options, [string] $Given, [string] $Question, [string] $Default)
    if ($Given) { return $Given }
    if ($Options.Unattended) { return $Default }
    $answer = Read-Host "$Question [$Default]"
    if ($answer) { return $answer.Trim() }
    return $Default
}

function Confirm-StatupChoice {
    param([hashtable] $Options, [string] $Question, [bool] $Default)
    if ($Options.Unattended) { return $Default }
    Test-StatupConsole
    $hint = if ($Default) { 'Y/n' } else { 'y/N' }
    $answer = Read-Host "$Question [$hint]"
    if (-not $answer) { return $Default }
    return $answer.Trim() -match '^(y|yes)$'
}

function Test-StatupFolder {
    param([string] $Path, [string] $Name, [switch] $AllowShare)
    if ($Path -match "['`"]") { throw "$Name ${Path}: a folder name cannot hold a quote" }
    $isShare = $Path -match '^\\\\[^\\]+\\[^\\]+'
    if ($isShare -and -not $AllowShare) { throw "$Name ${Path}: give a folder of this machine, not a share" }
    if (-not $isShare -and $Path -notmatch '^[A-Za-z]:\\') { throw "$Name ${Path}: give a full path, such as C:\Statup" }
    $system = @('', $env:SystemRoot, $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:ProgramData, (Split-Path $env:PUBLIC))
    $trimmed = $Path.TrimEnd('\')
    if ($trimmed -match '^[A-Za-z]:$' -or ($system | Where-Object { $_ -and $_.TrimEnd('\') -eq $trimmed })) {
        throw "$Name ${Path}: choose a folder of its own, not a system folder"
    }
}

function ConvertTo-StatupHost {
    param([string] $Listen)
    switch ($Listen) {
        'all' { return '0.0.0.0' }
        'local' { return '127.0.0.1' }
    }
    $address = $null
    if (-not [System.Net.IPAddress]::TryParse($Listen, [ref] $address)) { throw "listen ${Listen}: give all, local or an address" }
    return $Listen
}

function ConvertTo-StatupAccount {
    param([string] $Account)
    switch ($Account) {
        'Virtual' { return 'NT SERVICE\Statup' }
        'LocalSystem' { return 'LocalSystem' }
    }
    # .\name is a local account, which not every tool of Windows reads.
    if ($Account.StartsWith('.\')) { return "$env:COMPUTERNAME\" + $Account.Substring(2) }
    return $Account
}

# Only an account with a password needs it: a virtual account, the system
# and a group managed service account (ending with $) have none.
function Get-StatupCredential {
    param([hashtable] $Options, [string] $Account)
    if ($Account -in @('NT SERVICE\Statup', 'LocalSystem') -or $Account.EndsWith('$')) { return $null }
    if ($Options.Credential) { return $Options.Credential }
    if ($Options.Unattended) { throw "the account $Account needs its password: pass -Credential" }
    Test-StatupConsole
    return Get-Credential -UserName $Account -Message "Password of $Account, handed to Windows only"
}

function Get-StatupNetworkProfile {
    $profiles = @(Get-NetConnectionProfile -ErrorAction SilentlyContinue | ForEach-Object {
            if ($_.NetworkCategory -eq 'DomainAuthenticated') { 'Domain' } else { $_.NetworkCategory.ToString() }
        } | Select-Object -Unique)
    if ($profiles.Count -eq 0) { return @('Domain') }
    return $profiles
}

function Read-StatupFirewall {
    param([hashtable] $Options, [hashtable] $Choices)
    $Choices.OpenFirewall = $false
    if ($Choices.Host -in @('127.0.0.1', '::1') -or $Options.NoFirewall) { return }
    if (-not $Options.Firewall -and -not (Confirm-StatupChoice $Options "Open port $($Choices.Port) in Windows Firewall?" $true)) { return }
    $Choices.OpenFirewall = $true
    if ($Options.FirewallProfile) {
        $Choices.Profiles = $Options.FirewallProfile
        return
    }
    $answer = Read-StatupChoice $Options '' 'Firewall profiles: Domain, Private, Public' ((Get-StatupNetworkProfile) -join ', ')
    $Choices.Profiles = @($answer -split '[,\s]+' | Where-Object { $_ })
}

# setting <key>: its value in the settings file, without its quotes.
function Read-StatupSetting {
    param([string] $Path, [string] $Key)
    $value = $null
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        if ($line -match "^$Key=(.*)$") { $value = $Matches[1].Trim() }
    }
    if ($value -and $value.Length -ge 2 -and $value.StartsWith("'") -and $value.EndsWith("'")) {
        $value = $value.Substring(1, $value.Length - 2)
    }
    return $value
}

function Read-StatupSettingFile {
    param([hashtable] $Choices, [string] $Path)
    $database = Read-StatupSetting $Path 'DATABASE_URL'
    $database = $database -replace '^sqlite:(//)?', ''
    $Choices.DataDir = Split-Path -Parent $database
    $Choices.BackupDir = Read-StatupSetting $Path 'BACKUP_DIR'
    if (-not $Choices.BackupDir) { $Choices.BackupDir = [IO.Path]::Combine($Choices.DataDir, 'backups') }
    $Choices.Port = Read-StatupSetting $Path 'PORT'
    if (-not $Choices.Port) { $Choices.Port = '3000' }
    $Choices.Host = Read-StatupSetting $Path 'HOST'
    if (-not $Choices.Host) { $Choices.Host = '0.0.0.0' }
}

function Read-StatupInstallChoice {
    param([hashtable] $Options)
    if (-not $Options.Unattended) { Test-StatupConsole }
    $choices = @{}
    $choices.InstallDir = Read-StatupChoice $Options $Options.InstallDir 'Program folder' ([IO.Path]::Combine($env:ProgramFiles, 'Statup'))
    $settings = Get-StatupSettingsPath
    if (Test-Path $settings) {
        Write-Host "Keeping the settings of $settings"
        Read-StatupSettingFile $choices $settings
    }
    else {
        $choices.DataDir = Read-StatupChoice $Options $Options.DataDir 'Data folder' ([IO.Path]::Combine($env:ProgramData, 'Statup'))
        $choices.BackupDir = Read-StatupChoice $Options $Options.BackupDir 'Backup folder, a share such as \\nas\statup works' ([IO.Path]::Combine($choices.DataDir, 'backups'))
        $choices.Port = Read-StatupChoice $Options $Options.Port 'Port' '3000'
        $listenAnswer = Read-StatupChoice $Options $Options.Listen 'Listen on every address (all), or on this machine only, behind a reverse proxy (local)' 'all'
        $choices.Host = ConvertTo-StatupHost $listenAnswer
    }
    $accountDefault = if ($Options.Credential) { $Options.Credential.UserName } else { 'Virtual' }
    $accountAnswer = Read-StatupChoice $Options $Options.ServiceAccount 'Account of the service: Virtual, LocalSystem, or an account such as DOMAIN\statup$' $accountDefault
    $choices.Account = ConvertTo-StatupAccount $accountAnswer
    $choices.Credential = Get-StatupCredential $Options $choices.Account
    Test-StatupInstallChoice $choices
    Read-StatupFirewall $Options $choices
    return $choices
}

function Test-StatupInstallChoice {
    param([hashtable] $Choices)
    Test-StatupFolder $Choices.InstallDir 'program folder'
    Test-StatupFolder $Choices.DataDir 'data folder'
    Test-StatupFolder $Choices.BackupDir 'backup folder' -AllowShare
    $number = 0
    if (-not [int]::TryParse($Choices.Port, [ref] $number) -or $number -lt 1 -or $number -gt 65535) {
        throw "port $($Choices.Port): give a number from 1 to 65535"
    }
}

# The version GitHub's release page points to as the latest, read from its
# redirect: the API answers an office or a runner behind one address with a
# limit of 60 requests an hour.
function Get-StatupLatestVersion {
    param([string] $Repository)
    $request = [System.Net.HttpWebRequest]::Create("$Repository/releases/latest")
    $request.Method = 'HEAD'
    $request.AllowAutoRedirect = $false
    try { $response = $request.GetResponse() }
    catch { throw "cannot reach ${Repository}: pass -Version, or -Archive" }
    try { $location = $response.Headers['Location'] }
    finally { $response.Close() }
    if ($location -notmatch '/tag/v([0-9A-Za-z.-]+)$') { throw 'cannot tell the latest version of Statup: pass -Version' }
    return $Matches[1]
}

function Save-StatupFile {
    param([string] $Url, [string] $Path)
    try { Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing }
    catch { throw "cannot download ${Url}: $($_.Exception.Message)" }
}

function Test-StatupChecksum {
    param([string] $File, [string] $Sums)
    $name = Split-Path -Leaf $File
    $pattern = '^([0-9a-f]{64}) [ *]' + [regex]::Escape($name) + '$'
    $line = Get-Content $Sums | Where-Object { $_ -match $pattern } | Select-Object -First 1
    $actual = (Get-FileHash -Algorithm SHA256 -Path $File).Hash.ToLowerInvariant()
    if (-not $line -or $actual -ne ($line -split ' ')[0]) {
        throw "the checksum of $name does not match: download refused"
    }
}

# The archive of the version asked, its checksum verified: its path and
# version.
function Get-StatupArchive {
    param([hashtable] $Options, [string] $Temp)
    $archive = $Options.Archive
    if ($archive) {
        if (-not (Test-Path -PathType Leaf $archive)) { throw "${archive}: no such file" }
        if ((Split-Path -Leaf $archive) -notmatch '^statup-(.+)-windows-x86_64\.zip$') {
            throw "$archive is not a Statup archive for Windows"
        }
        return @{ Path = (Resolve-Path $archive).Path; Version = $Matches[1] }
    }
    $repository = 'https://github.com/karl-cta/statup'
    $wanted = "$($Options.Version)".TrimStart('v')
    if (-not $wanted) {
        if ($Options.Mirror) { throw '-Mirror needs -Version' }
        $wanted = Get-StatupLatestVersion $repository
    }
    $name = "statup-$wanted-windows-x86_64.zip"
    $from = if ($Options.Mirror) { "$($Options.Mirror)".TrimEnd('/') } else { "$repository/releases/download" }
    Write-Host "Downloading Statup $wanted"
    $file = [IO.Path]::Combine($Temp, $name)
    $sums = [IO.Path]::Combine($Temp, 'SHA256SUMS')
    Save-StatupFile "$from/v$wanted/$name" $file
    Save-StatupFile "$from/v$wanted/SHA256SUMS" $sums
    Test-StatupChecksum $file $sums
    return @{ Path = $file; Version = $wanted }
}

function Expand-StatupArchive {
    param([string] $Path, [string] $Temp)
    $target = [IO.Path]::Combine($Temp, 'unpacked')
    try { Expand-Archive -Path $Path -DestinationPath $target }
    catch { throw "$Path is not a Statup archive" }
    $folder = Get-ChildItem $target -Directory -Filter 'statup-*' |
        Where-Object { (Test-Path ([IO.Path]::Combine($_.FullName, 'statup.exe'))) -and (Test-Path ([IO.Path]::Combine($_.FullName, 'static'))) } |
        Select-Object -First 1
    if (-not $folder) { throw "$Path is not a Statup archive" }
    return $folder.FullName
}

function Install-StatupProgram {
    param([string] $Source, [string] $Folder, [string] $Release)
    New-Item -ItemType Directory -Force -Path $Folder | Out-Null
    Copy-Item ([IO.Path]::Combine($Source, 'statup.exe')) $Folder -Force
    $static = [IO.Path]::Combine($Folder, 'static')
    if (Test-Path $static) { Remove-Item $static -Recurse -Force }
    Copy-Item ([IO.Path]::Combine($Source, 'static')) $static -Recurse
    foreach ($file in @('README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md')) {
        Copy-Item ([IO.Path]::Combine($Source, $file)) $Folder -Force
    }
    [IO.File]::WriteAllText(([IO.Path]::Combine($Folder, 'VERSION')), "$Release`r`n")
}

# Only the files this script installed leave the program folder.
function Remove-StatupProgram {
    param([string] $Folder)
    foreach ($item in @('statup.exe', 'static', 'VERSION', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md')) {
        Remove-Item ([IO.Path]::Combine($Folder, $item)) -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (-not (Get-ChildItem $Folder -Force -ErrorAction SilentlyContinue)) {
        Remove-Item $Folder -Force -ErrorAction SilentlyContinue
    }
}

function Remove-StatupService {
    if (-not (Get-Service Statup -ErrorAction SilentlyContinue)) { return }
    Stop-Service Statup -ErrorAction SilentlyContinue
    & sc.exe delete Statup | Out-Null
    Get-NetFirewallRule -Name 'Statup' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
}

# Administrators and the system in full, the service account with $Right,
# nobody else: ProgramData lets every user read and add files. Groups by
# their SID, the same in every language of Windows.
function Set-StatupFolderAccess {
    param([string] $Path, [string] $Account, [string] $Right)
    if ($Path.StartsWith('\\')) {
        Write-Host "The share $Path must let $Account write to it: set that on the share"
        return
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    $grants = @('*S-1-5-32-544:(OI)(CI)F', '*S-1-5-18:(OI)(CI)F')
    if ($Account -ne 'LocalSystem') { $grants += "${Account}:(OI)(CI)$Right" }
    & icacls.exe $Path /inheritance:r /grant:r $grants | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cannot set who may use $Path" }
}

function Set-StatupAccess {
    param([hashtable] $Choices)
    Set-StatupFolderAccess $Choices.DataDir $Choices.Account 'M'
    Set-StatupFolderAccess ([IO.Path]::Combine($Choices.DataDir, 'uploads')) $Choices.Account 'M'
    Set-StatupFolderAccess $Choices.BackupDir $Choices.Account 'M'
    $settingsFolder = Split-Path -Parent (Get-StatupSettingsPath)
    if ($settingsFolder.TrimEnd('\') -ne $Choices.DataDir.TrimEnd('\')) {
        Set-StatupFolderAccess $settingsFolder $Choices.Account 'RX'
    }
}

# Paths between single quotes: the settings reader takes a backslash
# outside them as the start of an escape.
function Write-StatupSettingFile {
    param([hashtable] $Choices)
    $lines = @(
        '# Statup settings, read when it starts. After a change: Restart-Service Statup',
        '# Write paths between single quotes. Every setting: https://statup.dev/docs/configuration/',
        "HOST=$($Choices.Host)",
        "PORT=$($Choices.Port)",
        "DATABASE_URL='$([IO.Path]::Combine($Choices.DataDir, 'statup.db'))'",
        "UPLOAD_DIR='$([IO.Path]::Combine($Choices.DataDir, 'uploads'))'",
        "BACKUP_DIR='$($Choices.BackupDir)'"
    )
    [IO.File]::WriteAllLines((Get-StatupSettingsPath), [string[]] $lines)
}

# What services.msc does for an account it is given: Windows refuses to
# start a service as an account without the right to log on as a service.
function Grant-StatupServiceLogon {
    param([string] $Account)
    if (-not ('StatupServiceLogon' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;

public static class StatupServiceLogon {
    [StructLayout(LayoutKind.Sequential)]
    struct LsaString { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }

    [StructLayout(LayoutKind.Sequential)]
    struct LsaAttributes { public int Length; public IntPtr RootDirectory; public IntPtr ObjectName; public uint Attributes; public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService; }

    [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr system, ref LsaAttributes attributes, uint access, out IntPtr policy);
    [DllImport("advapi32.dll")] static extern uint LsaAddAccountRights(IntPtr policy, byte[] sid, LsaString[] rights, uint count);
    [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr policy);
    [DllImport("advapi32.dll")] static extern int LsaNtStatusToWinError(uint status);

    public static void Grant(string account) {
        var sid = (SecurityIdentifier)new NTAccount(account).Translate(typeof(SecurityIdentifier));
        var bytes = new byte[sid.BinaryLength];
        sid.GetBinaryForm(bytes, 0);
        var attributes = new LsaAttributes();
        IntPtr policy;
        uint status = LsaOpenPolicy(IntPtr.Zero, ref attributes, 0x810, out policy);
        if (status != 0) throw new Win32Exception(LsaNtStatusToWinError(status));
        var right = new LsaString();
        right.Buffer = Marshal.StringToHGlobalUni("SeServiceLogonRight");
        right.Length = (ushort)("SeServiceLogonRight".Length * 2);
        right.MaximumLength = (ushort)(right.Length + 2);
        try {
            status = LsaAddAccountRights(policy, bytes, new[] { right }, 1);
            if (status != 0) throw new Win32Exception(LsaNtStatusToWinError(status));
        } finally {
            Marshal.FreeHGlobal(right.Buffer);
            LsaClose(policy);
        }
    }
}
'@
    }
    [StatupServiceLogon]::Grant($Account)
}

# Windows restarts a service that stops on an error: three times, then
# counts again after a day.
function New-StatupService {
    param([hashtable] $Choices)
    $exe = [IO.Path]::Combine($Choices.InstallDir, 'statup.exe')
    $service = @{
        Name           = 'Statup'
        DisplayName    = 'Statup'
        Description    = 'Statup status page'
        BinaryPathName = "`"$exe`" --env `"$(Get-StatupSettingsPath)`" service"
        StartupType    = 'Automatic'
    }
    if ($Choices.Credential) { $service.Credential = $Choices.Credential }
    New-Service @service | Out-Null
    # A virtual or managed account has no password; WMI refuses the former,
    # sc.exe sets both.
    if ($Choices.Account -eq 'NT SERVICE\Statup' -or $Choices.Account.EndsWith('$')) {
        & sc.exe config Statup obj= $Choices.Account | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "cannot run the service as $($Choices.Account) (sc.exe error $LASTEXITCODE)" }
    }
    if ($Choices.Account -notin @('NT SERVICE\Statup', 'LocalSystem')) { Grant-StatupServiceLogon $Choices.Account }
    & sc.exe failure Statup reset= 86400 actions= restart/5000/restart/5000/restart/60000 | Out-Null
    & sc.exe failureflag Statup 1 | Out-Null
}

function Open-StatupPort {
    param([string] $Number, [string[]] $Profiles)
    Get-NetFirewallRule -Name 'Statup' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -Name 'Statup' -DisplayName 'Statup' -Direction Inbound -Protocol TCP -LocalPort $Number -Action Allow -Profile $Profiles | Out-Null
    Write-Host "Opened port $Number in Windows Firewall ($($Profiles -join ', '))"
}

function Get-StatupProbeAddress {
    param([string] $Address)
    switch ($Address) {
        '0.0.0.0' { return '127.0.0.1' }
        '::' { return '[::1]' }
    }
    if ($Address.Contains(':')) { return "[$Address]" }
    return $Address
}

# Waits up to 30 s for Statup to answer on the address it listens on.
function Wait-StatupHealth {
    param([hashtable] $Choices)
    $url = "http://$(Get-StatupProbeAddress $Choices.Host):$($Choices.Port)/health"
    for ($try = 0; $try -lt 30; $try++) {
        try {
            Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 5 | Out-Null
            return
        }
        catch { Start-Sleep -Seconds 1 }
    }
    throw "Statup does not answer on port $($Choices.Port): see statup.log in $($Choices.DataDir)"
}

function Get-StatupAddress {
    param([hashtable] $Choices)
    if ($Choices.Host -in @('0.0.0.0', '::')) {
        $name = [System.Net.Dns]::GetHostEntry('').HostName
        return "http://${name}:$($Choices.Port)"
    }
    return "http://$(Get-StatupProbeAddress $Choices.Host):$($Choices.Port)"
}

# Leaves the machine as it was, so that the next run installs again rather
# than updates a service that never started. The folders stay, empty.
function Undo-StatupInstall {
    param([hashtable] $Choices, [bool] $NewSettings)
    Write-Host 'The install stopped: removing what it had set up'
    Remove-StatupService
    Remove-StatupProgram $Choices.InstallDir
    if ($NewSettings) { Remove-Item (Get-StatupSettingsPath) -Force -ErrorAction SilentlyContinue }
}

function Install-Statup {
    param([hashtable] $Options, [string] $Temp)
    $choices = Read-StatupInstallChoice $Options
    $release = Get-StatupArchive $Options $Temp
    $source = Expand-StatupArchive $release.Path $Temp
    Write-Host "Installing Statup $($release.Version) in $($choices.InstallDir)"
    $newSettings = -not (Test-Path (Get-StatupSettingsPath))
    try {
        Install-StatupProgram $source $choices.InstallDir $release.Version
        New-StatupService $choices
        Set-StatupAccess $choices
        if ($newSettings) { Write-StatupSettingFile $choices }
        if ($choices.OpenFirewall) { Open-StatupPort $choices.Port $choices.Profiles }
        Start-Service Statup
    }
    catch {
        Undo-StatupInstall $choices $newSettings
        throw
    }
    Wait-StatupHealth $choices
    Write-Host ''
    Write-Host "Statup $($release.Version) is running: $(Get-StatupAddress $choices)"
    Write-Host "Open it to create the administrator account. Settings: $(Get-StatupSettingsPath)"
    Write-Host 'Help: https://statup.dev/docs/'
}

# The folders and settings of the installed service, read from its command
# line and its settings file.
function Read-StatupInstalled {
    $service = Get-CimInstance Win32_Service -Filter "Name='Statup'"
    if ($service.PathName -notmatch '^"(.+)\\statup\.exe" --env "(.+)" service$') {
        throw 'the Statup service was not installed by this script'
    }
    $choices = @{ InstallDir = $Matches[1]; Settings = $Matches[2]; Account = $service.StartName }
    if (Test-Path $choices.Settings) { Read-StatupSettingFile $choices $choices.Settings }
    return $choices
}

function Test-StatupDowngrade {
    param([hashtable] $Options, [string] $Folder, [string] $Release)
    $file = [IO.Path]::Combine($Folder, 'VERSION')
    if (-not (Test-Path $file)) { return }
    $installed = (Get-Content $file -TotalCount 1).Trim()
    $old = $null
    $new = $null
    if (-not [version]::TryParse($installed, [ref] $old) -or -not [version]::TryParse($Release, [ref] $new)) { return }
    if ($new -ge $old) { return }
    if (-not (Confirm-StatupChoice $Options "Statup $installed is installed: install the older $Release? Its database may be too new for it." $false)) {
        throw "kept Statup $installed"
    }
}

function Update-Statup {
    param([hashtable] $Options, [string] $Temp)
    if ($Options.InstallDir -or $Options.DataDir -or $Options.BackupDir -or $Options.Port -or $Options.Listen -or $Options.ServiceAccount) {
        Write-Host 'Statup is installed: the parameters of a new install are ignored, its settings are in its settings file'
    }
    $choices = Read-StatupInstalled
    if (-not $choices.DataDir) { throw "$($choices.Settings) is missing: remove Statup with -Uninstall, then install it again" }
    Write-Host "Updating Statup in $($choices.InstallDir), with its settings"
    $release = Get-StatupArchive $Options $Temp
    $source = Expand-StatupArchive $release.Path $Temp
    Test-StatupDowngrade $Options $choices.InstallDir $release.Version
    Stop-Service Statup -ErrorAction SilentlyContinue
    Install-StatupProgram $source $choices.InstallDir $release.Version
    foreach ($folder in @($choices.DataDir, $choices.BackupDir)) {
        if (-not (Test-Path $folder)) { Set-StatupFolderAccess $folder $choices.Account 'M' }
    }
    Get-NetFirewallRule -Name 'Statup' -ErrorAction SilentlyContinue | Set-NetFirewallRule -LocalPort $choices.Port
    Start-Service Statup
    Wait-StatupHealth $choices
    Write-Host "Statup $($release.Version) is running: $(Get-StatupAddress $choices)"
}

function Uninstall-Statup {
    param([hashtable] $Options)
    if (-not (Get-Service Statup -ErrorAction SilentlyContinue)) {
        Write-Host 'Statup is not installed as a service here'
        return
    }
    $choices = Read-StatupInstalled
    if (-not $Options.Unattended -and -not (Confirm-StatupChoice $Options "Remove Statup from $($choices.InstallDir)? Its data and settings stay." $false)) {
        throw 'nothing removed'
    }
    Remove-StatupService
    Remove-StatupProgram $choices.InstallDir
    $kept = if ($choices.DataDir) { "the data in $($choices.DataDir) and $($choices.BackupDir), the settings in $($choices.Settings)" } else { 'the data and settings' }
    Write-Host "Statup is removed. Kept: $kept."
}

function Test-StatupAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal $identity
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-StatupInstaller {
    param([hashtable] $Options)
    # Set here, not at the top: run through iex, the script shares the
    # session of the administrator.
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    if ([Environment]::OSVersion.Platform -ne 'Win32NT') { throw 'this script installs Statup on Windows; on Linux, use install.sh' }
    if (-not (Test-StatupAdministrator)) { throw 'run PowerShell as an administrator' }
    $temp = [IO.Path]::Combine([IO.Path]::GetTempPath(), 'statup-' + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $temp | Out-Null
    try {
        if ($Options.Uninstall) { Uninstall-Statup $Options }
        elseif (Get-Service Statup -ErrorAction SilentlyContinue) { Update-Statup $Options $temp }
        else { Install-Statup $Options $temp }
    }
    finally {
        Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# Last, so that a download cut short runs nothing.
Invoke-StatupInstaller @{
    InstallDir      = $InstallDir
    DataDir         = $DataDir
    BackupDir       = $BackupDir
    Port            = $Port
    Listen          = $Listen
    ServiceAccount  = $ServiceAccount
    Credential      = $Credential
    Firewall        = [bool] $Firewall
    NoFirewall      = [bool] $NoFirewall
    FirewallProfile = $FirewallProfile
    Version         = $Version
    Archive         = $Archive
    Mirror          = $Mirror
    Unattended      = [bool] $Unattended
    Uninstall       = [bool] $Uninstall
}
