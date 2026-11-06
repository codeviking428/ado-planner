<#
.SYNOPSIS
Install-test the Rust and Electron NSIS installers on a Windows runner.

.DESCRIPTION
For each app: install the last per-user release (v0.2.1), install the new
per-machine build over it, and check that it lands in Program Files, that the
per-user copy is gone, and that user data survives. Then uninstall it.
GitHub's Windows runners run as an administrator with UAC off, so this covers
paths, registry, and migration, not the UAC prompt itself.

.EXAMPLE
./test-windows-installers.ps1 -Version 0.2.2 -RustInstaller dist/rust/ado-planner-v0.2.2-x86_64-setup.exe -ElectronInstaller dist/electron/ado-planner-electron-0.2.2-setup.exe
#>
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $RustInstaller,
  [Parameter(Mandatory)] [string] $ElectronInstaller,
  [string] $BaselineTag = 'v0.2.1'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = 'https://github.com/codeviking428/ado-planner/releases/download'
$uninstallRoot = 'Software\Microsoft\Windows\CurrentVersion\Uninstall'
$dataDir = Join-Path $env:APPDATA 'ado-planner'
$marker = Join-Path $dataDir 'installer-test-marker.txt'

function Assert([bool] $Condition, [string] $Message) {
  if (-not $Condition) { throw "FAILED: $Message" }
  Write-Output "ok: $Message"
}

function Invoke-Installer([string] $Path, [string[]] $Arguments) {
  $process = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru
  Assert ($process.ExitCode -eq 0) "$(Split-Path -Leaf $Path) $($Arguments -join ' ') exits 0 (got $($process.ExitCode))"
}

function Get-Baseline([string] $Name) {
  $path = Join-Path $env:RUNNER_TEMP $Name
  Invoke-WebRequest -Uri "$repo/$BaselineTag/$Name" -OutFile $path
  return $path
}

function Find-UninstallKey([string] $Hive, [scriptblock] $Match) {
  $root = "${Hive}:\$uninstallRoot"
  if (-not (Test-Path $root)) { return $null }
  Get-ChildItem $root | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object $Match | Select-Object -First 1
}

function Get-StdoutLine([string] $Exe, [string] $Argument) {
  # Redirecting stdout is what the release workflow does, and it works for a
  # GUI-subsystem binary because the pipe handle is inherited.
  return ((& $Exe $Argument | Out-String).Trim())
}

New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
Set-Content -Path $marker -Value 'keep me'

Write-Output '== Rust: per-user baseline, then per-machine install'
$rustUser = Join-Path $env:LOCALAPPDATA 'ADO Planner'
$rustMachine = Join-Path $env:ProgramFiles 'ADO Planner'
$rustKey = "$uninstallRoot\ADO Planner"
Invoke-Installer (Get-Baseline "ado-planner-$BaselineTag-x86_64-setup.exe") @('/S')
Assert (Test-Path "$rustUser\ado-planner.exe") "baseline installs per-user to $rustUser"
Assert (Test-Path "HKCU:\$rustKey") 'baseline registers under HKCU'

Invoke-Installer (Resolve-Path $RustInstaller) @('/S')
Assert (Test-Path "$rustMachine\ado-planner.exe") "new build installs to $rustMachine"
$machine = Get-ItemProperty "HKLM:\$rustKey"
Assert ($machine.DisplayVersion -eq $Version) "HKLM uninstall entry has version $Version"
Assert (-not (Test-Path "HKCU:\$rustKey")) 'per-user uninstall entry is gone'
Assert (-not (Test-Path "$rustUser\ado-planner.exe")) 'per-user program files are gone'
Assert (Test-Path $marker) "user data in $dataDir survives"
python "$PSScriptRoot/check-pe-subsystem.py" "$rustMachine\ado-planner.exe" gui
Assert ($LASTEXITCODE -eq 0) 'installed ado-planner.exe uses the GUI subsystem'
$got = Get-StdoutLine "$rustMachine\ado-planner.exe" '--version'
Assert ($got -eq "ado-planner $Version") "installed ado-planner.exe --version prints 'ado-planner $Version' (got '$got')"

Invoke-Installer "$rustMachine\uninstall.exe" @('/S', "_?=$rustMachine")
Assert (-not (Test-Path "$rustMachine\ado-planner.exe")) 'uninstall removes the app'
Assert (-not (Test-Path "HKLM:\$rustKey")) 'uninstall removes the HKLM entry'
Assert (Test-Path $marker) 'uninstall keeps user data'
Remove-Item -Recurse -Force $rustMachine -ErrorAction SilentlyContinue

Write-Output '== Electron: per-user baseline, then per-machine install'
$electronUser = Join-Path $env:LOCALAPPDATA 'Programs\ado-planner'
$electronMachine = Join-Path $env:ProgramFiles 'ado-planner'
$isElectron = { $_.PSObject.Properties['UninstallString'] -and $_.UninstallString -like '*Uninstall ado-planner.exe*' }
Invoke-Installer (Get-Baseline "ado-planner-electron-$($BaselineTag.TrimStart('v'))-setup.exe") @('/S')
Assert (Test-Path "$electronUser\ado-planner.exe") "baseline installs per-user to $electronUser"
Assert ($null -ne (Find-UninstallKey HKCU $isElectron)) 'baseline registers under HKCU'

Invoke-Installer (Resolve-Path $ElectronInstaller) @('/S')
Assert (Test-Path "$electronMachine\ado-planner.exe") "new build installs to $electronMachine"
$entry = Find-UninstallKey HKLM $isElectron
Assert ($null -ne $entry -and $entry.DisplayVersion -eq $Version) "HKLM uninstall entry has version $Version"
Assert ($null -eq (Find-UninstallKey HKCU $isElectron)) 'per-user uninstall entry is gone'
Assert (-not (Test-Path "$electronUser\ado-planner.exe")) 'per-user program files are gone'
Assert (Test-Path $marker) "user data in $dataDir survives"

$quiet = $entry.QuietUninstallString
Assert (-not [string]::IsNullOrEmpty($quiet)) 'HKLM entry has a QuietUninstallString'
$process = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $quiet -Wait -PassThru
Assert ($process.ExitCode -eq 0) "quiet uninstall exits 0 (got $($process.ExitCode))"
# The uninstaller copies itself to %TEMP% and returns before it finishes.
for ($i = 0; $i -lt 30 -and (Test-Path "$electronMachine\ado-planner.exe"); $i++) { Start-Sleep -Seconds 2 }
Assert (-not (Test-Path "$electronMachine\ado-planner.exe")) 'uninstall removes the app'
Assert (Test-Path $marker) 'uninstall keeps user data'

Remove-Item -Force $marker
Write-Output 'Windows installer tests passed'
