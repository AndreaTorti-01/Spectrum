<#
.SYNOPSIS
  Repeatable screenshots of Spectrum's windows, in light and dark.

.DESCRIPTION
  For each theme: stops any running Spectrum, starts a tray instance with --theme, covers the primary work
  area with a solid backdrop (--debug-stage) so nothing from the desktop can end up in a picture, then
  opens each surface (main window, gamut window, tray menu) and saves the exact window rectangle
  (DWM frame bounds: no shadow, no backdrop) as <surface>-<theme>.png. hdr-<theme>.png is the HDR section
  cropped out of the main window. With -Demo it also records the scripted --debug-demo at about 10 fps.

  Visual capture only: nothing here changes a color setting (--debug-demo runs on a mock UI state).

.PARAMETER Theme  light, dark or both.
.PARAMETER Out    Output folder for the README pictures (relative to the repo root).
.PARAMETER SiteOut  Second output folder for the website.
.PARAMETER Scale  UI scale factors (x the real DPI scale, --debug-scale). Default 1 and 2. A pass at 1 writes
                  *-1x.png (README: GitHub shows images at pixel size) and the tray menu; any other pass writes the
                  plain names (site, hi-DPI). The tray menu is drawn by Windows and is only captured at 1x.
.PARAMETER Demo   Also record demo frames into screenshots-raw\demo-<theme>\frame-NNNN.png.
.PARAMETER Exe    Path to Spectrum.exe (default: target\release\Spectrum.exe).
#>
[CmdletBinding()]
param(
    [ValidateSet('light', 'dark', 'both')][string]$Theme = 'both',
    [string]$Out = 'docs\screenshots',
    [string]$SiteOut = 'site\src\shots',
    [double[]]$Scale = @(1, 2),
    [switch]$Demo,
    [string]$Exe = 'target\release\Spectrum.exe',
    [string]$RawOut = 'screenshots-raw'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
function Resolve-Repo([string]$p) { if ([System.IO.Path]::IsPathRooted($p)) { $p } else { Join-Path $root $p } }
$Exe = Resolve-Repo $Exe
$Out = Resolve-Repo $Out
$SiteOut = Resolve-Repo $SiteOut
$RawOut = Resolve-Repo $RawOut
if (-not (Test-Path $Exe)) { throw "Spectrum.exe not found at $Exe (run cargo build --release)" }

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;

public static class Cap
{
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);

    public static void Init() { SetProcessDPIAware(); }

    public static IntPtr Find(string cls) {
        IntPtr h = FindWindow(cls, null);
        return (h != IntPtr.Zero && IsWindowVisible(h)) ? h : IntPtr.Zero;
    }

    /// Visible frame of a window in physical pixels, without the DWM shadow.
    public static int[] FrameRect(IntPtr h) {
        RECT r;
        if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) != 0) GetWindowRect(h, out r);
        return new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top };
    }

    /// Client area origin and size in physical pixels.
    public static int[] ClientRect(IntPtr h) {
        RECT c; GetClientRect(h, out c);
        POINT p = new POINT(); ClientToScreen(h, ref p);
        return new int[] { p.X, p.Y, c.Right, c.Bottom };
    }

    public static Bitmap Grab(int x, int y, int w, int h) {
        Bitmap bmp = new Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format24bppRgb);
        using (Graphics g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(x, y, 0, 0, new Size(w, h), CopyPixelOperation.SourceCopy);
        }
        return bmp;
    }

    /// The 1 px DWM border of a window lets a little of what is behind the backdrop (wallpaper) shine through.
    /// Paint the outermost ring of the rectangle in its median shade (corners, which are rounded, only gray).
    public static void GrayRing(Bitmap b, int x, int y, int w, int h) {
        var pts = new System.Collections.Generic.List<int[]>();
        for (int i = 0; i < w; i++) { pts.Add(new int[] { x + i, y }); pts.Add(new int[] { x + i, y + h - 1 }); }
        for (int j = 1; j < h - 1; j++) { pts.Add(new int[] { x, y + j }); pts.Add(new int[] { x + w - 1, y + j }); }
        var lum = new System.Collections.Generic.List<int>();
        foreach (var p in pts) {
            if (p[0] < 0 || p[1] < 0 || p[0] >= b.Width || p[1] >= b.Height) continue;
            lum.Add(Luma(b.GetPixel(p[0], p[1])));
        }
        lum.Sort();
        int med = lum.Count > 0 ? lum[lum.Count / 2] : 0;
        const int corner = 12;
        foreach (var p in pts) {
            if (p[0] < 0 || p[1] < 0 || p[0] >= b.Width || p[1] >= b.Height) continue;
            bool nearCorner = (p[0] - x < corner || x + w - 1 - p[0] < corner) && (p[1] - y < corner || y + h - 1 - p[1] < corner);
            int g = nearCorner ? Luma(b.GetPixel(p[0], p[1])) : med;
            b.SetPixel(p[0], p[1], Color.FromArgb(g, g, g));
        }
    }
    static int Luma(Color c) { return (int)Math.Round(0.299 * c.R + 0.587 * c.G + 0.114 * c.B); }

    [DllImport("user32.dll", EntryPoint = "GetWindowLongW")] static extern int GetWindowLong(IntPtr h, int idx);
    public static bool IsTopmost(IntPtr h) { return (GetWindowLong(h, -20) & 0x8) != 0; }

    /// Number of pixels outside the given keep-rectangles (x,y,w,h) that differ from the backdrop color.
    public static int Stray(Bitmap b, int r, int g, int bl, int[][] keep) {
        var rect = new Rectangle(0, 0, b.Width, b.Height);
        var d = b.LockBits(rect, System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format24bppRgb);
        int n = 0;
        try {
            byte[] row = new byte[Math.Abs(d.Stride)];
            for (int y = 0; y < b.Height; y++) {
                Marshal.Copy(new IntPtr(d.Scan0.ToInt64() + (long)y * d.Stride), row, 0, row.Length);
                for (int x = 0; x < b.Width; x++) {
                    bool inside = false;
                    foreach (var k in keep) { if (x >= k[0] && x < k[0] + k[2] && y >= k[1] && y < k[1] + k[3]) { inside = true; break; } }
                    if (inside) continue;
                    int o = x * 3;
                    if (Math.Abs(row[o + 2] - r) + Math.Abs(row[o + 1] - g) + Math.Abs(row[o] - bl) > 6) n++;
                }
            }
        } finally { b.UnlockBits(d); }
        return n;
    }

    public static void Close(IntPtr h) { PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }

    public static void Escape(IntPtr menu) {
        PostMessage(menu, 0x0100, (IntPtr)0x1B, IntPtr.Zero);
        PostMessage(menu, 0x0101, (IntPtr)0x1B, IntPtr.Zero);
    }

    public static void EscapeKey() {
        keybd_event(0x1B, 0, 0, UIntPtr.Zero);
        keybd_event(0x1B, 0, 2, UIntPtr.Zero);
    }
}
'@
[Cap]::Init()
Add-Type -AssemblyName System.Drawing

function Stop-Spectrum {
    if (Test-Path $Exe) { try { & $Exe --quit } catch {} }
    Get-Process -Name Spectrum -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 400
}

function Wait-Window([string]$Class, [int]$TimeoutMs = 8000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $h = [Cap]::Find($Class)
        if ($h -ne [IntPtr]::Zero) { return $h }
        Start-Sleep -Milliseconds 50
    }
    throw "window of class $Class did not appear within $TimeoutMs ms"
}

function Wait-Gone([string]$Class, [int]$TimeoutMs = 4000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        if ([Cap]::Find($Class) -eq [IntPtr]::Zero) { return $true }
        Start-Sleep -Milliseconds 50
    }
    return $false
}

function Save-Png([System.Drawing.Bitmap]$Bmp, [string]$Path) {
    $Bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

# Capture the exact rectangle of a window; returns @{ Rect = x,y,w,h; Bitmap = ...; Client = x,y,w,h }
function Get-WindowShot([IntPtr]$Hwnd) {
    $r = [Cap]::FrameRect($Hwnd)
    $c = [Cap]::ClientRect($Hwnd)
    $bmp = [Cap]::Grab($r[0], $r[1], $r[2], $r[3])
    [Cap]::GrayRing($bmp, 0, 0, $r[2], $r[3])
    @{ Rect = $r; Client = $c; Bitmap = $bmp }
}

function Open-Surface([string]$Name) {
    # The showcase variants run on a mock state (EDID profile selected, HDR on) or an in-memory gamut entry;
    # nothing is applied and no file is written.
    $arg = @{ main = 'main-hdr'; gamut = 'gamut-showcase'; traymenu = 'traymenu-showcase' }[$Name]
    & $Exe --debug-open $arg | Out-Null
}

$themes = if ($Theme -eq 'both') { @('light', 'dark') } else { @($Theme) }
New-Item -ItemType Directory -Force -Path $Out, $SiteOut | Out-Null

$classes = @{ main = 'SpectrumMainWindow'; gamut = 'SpectrumGamutWindow'; traymenu = '#32768' }

foreach ($t in $themes) {
    $rects = @{}
    foreach ($sf in $Scale) {
        $one = ($sf -eq 1)
        $suffix = if ($one) { '-1x' } else { '' }
        Write-Host "== $t, scale $sf =="
        Stop-Spectrum

        $stage = Start-Process -FilePath $Exe -ArgumentList '--debug-stage', $t -PassThru
        Wait-Window 'SpectrumStage' | Out-Null
        $trayArgs = @('--tray', '--theme', $t)
        if (-not $one) { $trayArgs += @('--debug-scale', "$sf") }
        $tray = Start-Process -FilePath $Exe -ArgumentList $trayArgs -PassThru
        Start-Sleep -Milliseconds 1500

        $surfaces = if ($one) { @('gamut', 'traymenu', 'main') } else { @('gamut', 'main') }
        foreach ($s in $surfaces) {
            Open-Surface $s
            $h = Wait-Window $classes[$s]
            Start-Sleep -Milliseconds 600
            if ($s -ne 'traymenu' -and -not [Cap]::IsTopmost($h)) { throw "$s is not above the backdrop; refusing to capture" }
            $shot = Get-WindowShot $h
            if ($one) { $rects[$s] = $shot.Rect }
            if ($s -eq 'traymenu') {
                Save-Png $shot.Bitmap (Join-Path $Out "traymenu-$t.png")
                Save-Png $shot.Bitmap (Join-Path $Out "traymenu-$t-1x.png")
            } else {
                Save-Png $shot.Bitmap (Join-Path $Out "$s-$t$suffix.png")
            }
            Write-Host ("  {0}: {1}x{2} at {3},{4}" -f $s, $shot.Rect[2], $shot.Rect[3], $shot.Rect[0], $shot.Rect[1])

            if ($s -eq 'main') {
                # HDR section: client rows 308..550 of the 440 x 620 logical layout, full client width.
                $k = $shot.Client[2] / 440.0
                $cx = $shot.Client[0] - $shot.Rect[0]
                $cy = $shot.Client[1] - $shot.Rect[1]
                $y0 = $cy + [int][Math]::Round(308 * $k)
                $y1 = $cy + [int][Math]::Round(550 * $k)
                $rect = New-Object System.Drawing.Rectangle $cx, $y0, $shot.Client[2], ($y1 - $y0)
                $crop = $shot.Bitmap.Clone($rect, $shot.Bitmap.PixelFormat)
                Save-Png $crop (Join-Path $Out "hdr-$t$suffix.png")
                $crop.Dispose()
            }
            $shot.Bitmap.Dispose()

            if ($s -eq 'traymenu') { [Cap]::Escape($h); if (-not (Wait-Gone $classes[$s] 800)) { [Cap]::EscapeKey() } } else { [Cap]::Close($h) }
            if (-not (Wait-Gone $classes[$s])) { Write-Warning "$s did not close; stopping Spectrum"; Stop-Spectrum }
            Start-Sleep -Milliseconds 300
        }

        if ($Demo -and $one) {
            Stop-Spectrum
            $stage = Start-Process -FilePath $Exe -ArgumentList '--debug-stage', $t -PassThru
            Wait-Window 'SpectrumStage' | Out-Null
            # Union of the main window and the tray menu: both are inside the stage, nothing else is.
            $m = $rects['main']; $q = $rects['traymenu']
            $x0 = [Math]::Min($m[0], $q[0]); $y0 = [Math]::Min($m[1], $q[1])
            $x1 = [Math]::Max($m[0] + $m[2], $q[0] + $q[2]); $y1 = [Math]::Max($m[1] + $m[3], $q[1] + $q[3])
            # Never reach into the taskbar (clock, pinned apps, tray icons).
            $bar = [Cap]::Find('Shell_TrayWnd')
            if ($bar -ne [IntPtr]::Zero) {
                $br = [Cap]::FrameRect($bar)
                if ($br[1] -gt $y0) { $y1 = [Math]::Min($y1, $br[1]) }
            }
            $w = $x1 - $x0; $h = $y1 - $y0
            $dir = Join-Path $RawOut "demo-$t"
            if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $dir | Out-Null

            $demoProc = Start-Process -FilePath $Exe -ArgumentList '--debug-demo', '--theme', $t -PassThru
            Wait-Window '#32768' 8000 | Out-Null
            $frames = New-Object System.Collections.Generic.List[System.Drawing.Bitmap]
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $next = 0
            while (-not $demoProc.HasExited -and $sw.ElapsedMilliseconds -lt 14000) {
                if ($sw.ElapsedMilliseconds -ge $next) {
                    $frames.Add([Cap]::Grab($x0, $y0, $w, $h))
                    $next += 100
                } else {
                    Start-Sleep -Milliseconds 5
                }
            }
            # Privacy check: outside the main window and the menu (plus their shadows) only the backdrop may show.
            $sr, $sg, $sb = if ($t -eq 'dark') { 0x20, 0x20, 0x20 } else { 0xE9, 0xE6, 0xE1 }
            $keep = @(
                [int[]]@(($m[0] - $x0 - 80), ($m[1] - $y0 - 80), ($m[2] + 160), ($m[3] + 160)),
                [int[]]@(($q[0] - $x0 - 80), ($q[1] - $y0 - 80), ($q[2] + 160), ($q[3] + 160)))
            $bad = 0
            foreach ($f in $frames) { if ([Cap]::Stray($f, $sr, $sg, $sb, [int[][]]$keep) -gt 0) { $bad++ } }
            if ($bad -gt 0) {
                Save-Png $frames[[int]($frames.Count / 2)] (Join-Path $RawOut 'failed-frame.png')
                throw "$bad of $($frames.Count) demo frames contain something other than the backdrop; frames not saved (see failed-frame.png)"
            }
            $i = 1
            foreach ($f in $frames) {
                [Cap]::GrayRing($f, $m[0] - $x0, $m[1] - $y0, $m[2], $m[3])
                Save-Png $f (Join-Path $dir ('frame-{0:D4}.png' -f $i))
                $f.Dispose(); $i++
            }
            Write-Host ("  demo: {0} frames, {1}x{2} at {3},{4}" -f $frames.Count, $w, $h, $x0, $y0)
        }

        Stop-Spectrum
    }
}

# Same pictures for the website.
foreach ($t in $themes) {
    foreach ($f in Get-ChildItem (Join-Path $Out "*-$t*.png")) {
        Copy-Item -Force $f.FullName (Join-Path $SiteOut $f.Name)
    }
}
Write-Host "done: $Out and $SiteOut"
