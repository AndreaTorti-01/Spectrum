<#
.SYNOPSIS
  Builds the MSIX package for the Microsoft Store, or registers it locally for testing.

.DESCRIPTION
  1. cargo build --release (build.rs also writes target\msix-assets and target\store-tile.png)
  2. Lays out target\msix\: Spectrum.exe, Assets\, AppxManifest.xml with the version from Cargo.toml
  3. makepri: resources.pri, so the scale-* and targetsize-* images resolve
  4. -Register: Add-AppxPackage -Register on the folder (needs Developer Mode, no signing)
     otherwise: makeappx pack to target\Spectrum-X.Y.Z.msix (unsigned: the Store signs it)

  Needs the Windows SDK (makepri, makeappx): winget install Microsoft.WindowsSDK.10.0.26100

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1 -Register
#>
param(
    [switch]$Register,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Partner Center > Product management > Product identity. Case-sensitive, must match exactly.
$identity = @{
    IDENTITY_NAME          = 'Spectrum.Dev'
    IDENTITY_PUBLISHER     = 'CN=Spectrum Dev'
    PUBLISHER_DISPLAY_NAME = 'Andrea Torti'
}

function Get-CargoVersion {
    $line = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"' | Select-Object -First 1
    if (-not $line) { throw 'No version in Cargo.toml' }
    $g = $line.Matches[0].Groups
    if ([int]$g[1].Value -eq 0) { Write-Warning 'The Store rejects a version whose first field is 0.' }
    return "$($g[1].Value).$($g[2].Value).$($g[3].Value)"
}

function Find-SdkTool([string]$name) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $bin = 'C:\Program Files (x86)\Windows Kits\10\bin'
    if (Test-Path $bin) {
        $hit = Get-ChildItem $bin -Directory |
            Where-Object { $_.Name -match '^10\.' } |
            Sort-Object { [version]$_.Name } -Descending |
            ForEach-Object { Join-Path $_.FullName "x64\$name" } |
            Where-Object { Test-Path $_ } |
            Select-Object -First 1
        if ($hit) { return $hit }
    }
    return $null
}

$version = Get-CargoVersion
$msixVersion = "$version.0"

if (-not $SkipBuild) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}

$layout = Join-Path $root 'target\msix'
if (Test-Path $layout) { Remove-Item -Recurse -Force $layout }
New-Item -ItemType Directory -Force (Join-Path $layout 'Assets') | Out-Null
Copy-Item target\release\Spectrum.exe $layout
Copy-Item target\msix-assets\*.png (Join-Path $layout 'Assets')
Copy-Item LICENSE $layout

$manifest = Get-Content packaging\AppxManifest.xml -Raw
$manifest = $manifest.Replace('$VERSION$', $msixVersion)
foreach ($k in $identity.Keys) { $manifest = $manifest.Replace("`$$k`$", $identity[$k]) }
if ($manifest -match '\$[A-Z_]+\$') { throw "Unfilled placeholder in the manifest: $($Matches[0])" }
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

$makepri = Find-SdkTool 'makepri.exe'
if ($makepri) {
    $cfg = Join-Path $root 'target\priconfig.xml'
    & $makepri createconfig /cf $cfg /dq en-US /pv 10.0.0 /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'makepri createconfig failed' }
    & $makepri new /pr $layout /cf $cfg /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'makepri new failed' }
} elseif ($Register) {
    # Without resources.pri only exact file names resolve: give the manifest's plain names a copy.
    Write-Warning 'makepri.exe not found (Windows SDK); using unqualified asset copies for this local run.'
    foreach ($n in 'Square44x44Logo', 'Square150x150Logo', 'StoreLogo') {
        Copy-Item (Join-Path $layout "Assets\$n.scale-100.png") (Join-Path $layout "Assets\$n.png")
    }
} else {
    throw 'makepri.exe not found. Install the Windows SDK: winget install Microsoft.WindowsSDK.10.0.26100'
}

if ($Register) {
    Get-AppxPackage -Name $identity.IDENTITY_NAME | Remove-AppxPackage
    Add-AppxPackage -Register (Join-Path $layout 'AppxManifest.xml')
    $pkg = Get-AppxPackage -Name $identity.IDENTITY_NAME
    Write-Host "Registered $($pkg.PackageFullName) from $layout"
    Write-Host 'WACK (elevated shell, Spectrum not running):'
    Write-Host "  & 'C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe' reset"
    Write-Host "  & 'C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe' test -packagefullname $($pkg.PackageFullName) -reportoutputpath $root\target\wack.xml"
    return
}

$makeappx = Find-SdkTool 'makeappx.exe'
if (-not $makeappx) { throw 'makeappx.exe not found. Install the Windows SDK.' }
$out = Join-Path $root "target\Spectrum-$version.msix"
& $makeappx pack /o /d $layout /p $out | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makeappx pack failed' }
Write-Host "Packed $out ($msixVersion)"
