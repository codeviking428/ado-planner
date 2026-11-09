<#
.SYNOPSIS
Install-test the NSIS installer on a Windows runner.

.DESCRIPTION
Install the last per-user release (v0.2.1), install the new per-machine build
over it, and check that it lands in Program Files, that the per-user copy is
gone, and that user data survives. Then uninstall it.

Then, for each Electron release in -ElectronBaselineTags: install it, install
the new build, and check that the new build removed the Electron app but kept
its user data and left working shortcuts. This is the path electron-updater
takes when the Electron app installs the new build from its update prompt.

GitHub's Windows runners run as an administrator with UAC off, so this covers
paths, registry, and migration, not the UAC prompt itself.

.EXAMPLE
./test-windows-installers.ps1 -Version 1.0.0 -RustInstaller dist/rust/ado-planner-v1.0.0-x86_64-setup.exe
#>
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $RustInstaller,
  [string] $BaselineTag = 'v0.2.1',
  # Published Electron releases to migrate from. Add v0.3.0 if it ships with
  # the per-machine Electron installer.
  [string[]] $ElectronBaselineTags = @('v0.2.1')
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

function Get-Baseline([string] $Tag, [string] $Name) {
  $path = Join-Path $env:RUNNER_TEMP $Name
  Invoke-WebRequest -Uri "$repo/$Tag/$Name" -OutFile $path
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
Invoke-Installer (Get-Baseline $BaselineTag "ado-planner-$BaselineTag-x86_64-setup.exe") @('/S')
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

$isElectron = { $_.PSObject.Properties['QuietUninstallString'] -and $_.QuietUninstallString -like '*\Uninstall ado-planner.exe"*' }
$shell = New-Object -ComObject WScript.Shell
$commonDesktop = [Environment]::GetFolderPath('CommonDesktopDirectory')
$commonPrograms = [Environment]::GetFolderPath('CommonPrograms')
$shortcutRoots = @($commonDesktop, $commonPrograms) + @('DesktopDirectory', 'Programs' | ForEach-Object { [Environment]::GetFolderPath($_) })

function Get-Shortcuts {
  Get-ChildItem -Path $shortcutRoots -Recurse -Filter '*.lnk' -ErrorAction SilentlyContinue |
    ForEach-Object { [pscustomobject]@{ Path = $_.FullName; Target = $shell.CreateShortcut($_.FullName).TargetPath } }
}

foreach ($tag in $ElectronBaselineTags) {
  Write-Output "== Electron $tag, then the new build replaces it"
  Invoke-Installer (Get-Baseline $tag "ado-planner-electron-$($tag.TrimStart('v'))-setup.exe") @('/S')
  $electron = @('HKCU', 'HKLM') | ForEach-Object { Find-UninstallKey $_ $isElectron } | Select-Object -First 1
  Assert ($null -ne $electron) "Electron $tag registers an uninstall entry"
  $electronDir = $electron.QuietUninstallString.Split('"')[1] | Split-Path -Parent
  Assert (Test-Path "$electronDir\ado-planner.exe") "Electron $tag installs to $electronDir"

  Invoke-Installer (Resolve-Path $RustInstaller) @('/S')
  Assert (Test-Path "$rustMachine\ado-planner.exe") "new build installs to $rustMachine"
  Assert ($null -eq (Find-UninstallKey HKCU $isElectron)) 'no Electron uninstall entry under HKCU'
  Assert ($null -eq (Find-UninstallKey HKLM $isElectron)) 'no Electron uninstall entry under HKLM'
  Assert (-not (Test-Path "$electronDir\ado-planner.exe")) "Electron program files in $electronDir are gone"
  Assert (-not (Test-Path $electronDir)) "$electronDir is removed"
  Assert (Test-Path $marker) "user data in $dataDir survives"
  $shortcuts = @(Get-Shortcuts)
  $rustExe = "$rustMachine\ado-planner.exe"
  foreach ($root in @($commonDesktop, $commonPrograms)) {
    $found = $shortcuts | Where-Object { $_.Target -eq $rustExe -and $_.Path.StartsWith($root) }
    Assert ($null -ne $found) "a shortcut under $root opens $rustExe"
  }
  $stale = @($shortcuts | Where-Object { $_.Target -like "$electronDir\*" } | ForEach-Object Path)
  Assert ($stale.Count -eq 0) "no shortcut opens the Electron app (found: $($stale -join ', '))"

  Invoke-Installer "$rustMachine\uninstall.exe" @('/S', "_?=$rustMachine")
  Assert (-not (Test-Path "$rustMachine\ado-planner.exe")) 'uninstall removes the app'
  Assert (Test-Path $marker) 'uninstall keeps user data'
  Remove-Item -Recurse -Force $rustMachine -ErrorAction SilentlyContinue
}

Remove-Item -Force $marker
Write-Output 'Windows installer tests passed'
