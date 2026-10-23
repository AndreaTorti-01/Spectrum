<#
.SYNOPSIS
  Renders Spectrum's screenshots and README demo frames off-screen, in light and dark.

.DESCRIPTION
  Every picture comes from `Spectrum.exe --render`: the windows' own drawing code paints into an off-screen
  bitmap and is saved as PNG. No window opens, nothing takes focus or draws on the screen, and a running
  Spectrum is left alone. The data is the fixed showcase state (src/showcase.rs), so the output is the same
  on any machine and at any display scale.

  Per theme <t>:
    <surface>-<t>.png      2x client area (website), for main, gamut, profiles and hdr
    <surface>-<t>-1x.png   1x with a drawn Windows 11 frame (README); hdr-<t>-1x.png is the 1x HDR crop
    traymenu-<t>.png       the tray menu replica at 1x (also copied as traymenu-<t>-1x.png)
  plus site\src\data\layout.json (main window callouts, HDR crop rows, gamut plot), and with -Demo the
  README demo frames in screenshots-raw\demo-<t>\frame-NNNN.png for `cd site; npm run demo`.

.PARAMETER Theme    light, dark or both.
.PARAMETER Out      Output folder for the README pictures (relative to the repo root).
.PARAMETER SiteOut  Second output folder for the website.
.PARAMETER Demo     Also render the demo frames.
.PARAMETER Exe      Path to Spectrum.exe (default: target\release\Spectrum.exe).
#>
[CmdletBinding()]
param(
    [ValidateSet('light', 'dark', 'both')][string]$Theme = 'both',
    [string]$Out = 'docs\screenshots',
    [string]$SiteOut = 'site\src\shots',
    [switch]$Demo,
    [string]$Exe = 'target\release\Spectrum.exe',
    [string]$RawOut = 'screenshots-raw',
    [string]$Layout = 'site\src\data\layout.json'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
function Resolve-Repo([string]$p) { if ([System.IO.Path]::IsPathRooted($p)) { $p } else { Join-Path $root $p } }
$Exe = Resolve-Repo $Exe
$Out = Resolve-Repo $Out
$SiteOut = Resolve-Repo $SiteOut
$RawOut = Resolve-Repo $RawOut
$Layout = Resolve-Repo $Layout
if (-not (Test-Path $Exe)) { throw "Spectrum.exe not found at $Exe (run cargo build --release)" }
New-Item -ItemType Directory -Force -Path $Out, $SiteOut | Out-Null

# The exe is GUI-subsystem: wait for it, check the exit code and the file it should have written.
function Render([string[]]$Arguments, [string]$Path) {
    if (Test-Path $Path) { Remove-Item $Path -Recurse -Force }
    $p = Start-Process -FilePath $Exe -ArgumentList ($Arguments + @('--out', "`"$Path`"")) -Wait -PassThru -WindowStyle Hidden
    if ($p.ExitCode -ne 0 -or -not (Test-Path $Path)) { throw "Spectrum.exe $($Arguments -join ' ') failed (exit $($p.ExitCode))" }
    Write-Host "  $Path"
}

$themes = if ($Theme -eq 'both') { @('light', 'dark') } else { @($Theme) }
foreach ($t in $themes) {
    Write-Host "== $t"
    foreach ($s in 'main', 'gamut', 'profiles', 'hdr') {
        Render @('--render', $s, '--theme', $t, '--scale', '2') (Join-Path $Out "$s-$t.png")
        $frame = if ($s -eq 'hdr') { @() } else { @('--frame') }
        Render (@('--render', $s, '--theme', $t, '--scale', '1') + $frame) (Join-Path $Out "$s-$t-1x.png")
    }
    $menu = Join-Path $Out "traymenu-$t.png"
    Render @('--render', 'traymenu', '--theme', $t, '--scale', '1') $menu
    Copy-Item -Force $menu (Join-Path $Out "traymenu-$t-1x.png")
    if ($Demo) {
        Render @('--render', 'demo', '--theme', $t, '--scale', '1') (Join-Path $RawOut "demo-$t")
    }
}
Render @('--render', 'layout') $Layout

# Same pictures for the website.
foreach ($t in $themes) {
    foreach ($f in Get-ChildItem (Join-Path $Out "*-$t*.png")) {
        Copy-Item -Force $f.FullName (Join-Path $SiteOut $f.Name)
    }
}
Write-Host "done: $Out and $SiteOut"
