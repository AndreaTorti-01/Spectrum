<#
.SYNOPSIS
  Repeatable screenshots of Spectrum's windows, in light and dark.

.DESCRIPTION
  For each theme: stops any running Spectrum, starts a tray instance with --theme, covers the primary work
  area with a solid backdrop (--debug-stage) so nothing from the desktop can end up in a picture, then
  opens each surface (main window, gamut window, tray menu) and saves the exact window rectangle
  (DWM frame bounds: no shadow, no backdrop) as <surface>-<theme>.png. hdr-<theme>.png is the HDR section
  cropped out of the main window. With -Demo it also records the scripted --debug-demo at 10 fps: the demo
  writes a layout file (capture region, rectangles, start time, cursor path), the script records exactly that
  region and draws an arrow cursor on the frames.

  Visual capture only: nothing here changes a color setting (--debug-demo runs on a mock UI state).

.PARAMETER Theme  light, dark or both.
.PARAMETER Out    Output folder for the README pictures (relative to the repo root).
.PARAMETER SiteOut  Second output folder for the website.
.PARAMETER Scale  UI scale factors (x the real DPI scale, --debug-scale). Default 1 and 2. A pass at 1 writes
                  full-window *-1x.png (title bar included) (README: GitHub shows images at pixel size) and the tray menu; any other pass writes the
                  plain names (site, hi-DPI) as client area only: the system title bar does not scale. The tray menu is drawn by Windows and is only captured at 1x.
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
using System.Drawing.Drawing2D;
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

    /// True when every pixel has the backdrop color.
    public static bool IsBlank(Bitmap b, int r, int g, int bl) { return Stray(b, r, g, bl, new int[0][]) == 0; }

    /// Draws an arrow cursor (white, dark outline, soft shadow) with its tip at (x, y).
    public static void Sprite(Bitmap b, float x, float y, float s) {
        float[][] a = { new float[] {0, 0}, new float[] {0, 16.5f}, new float[] {4.1f, 12.9f}, new float[] {6.9f, 19.2f},
                        new float[] {9.4f, 18.1f}, new float[] {6.7f, 11.9f}, new float[] {11.8f, 11.9f} };
        PointF[] p = new PointF[a.Length]; PointF[] q = new PointF[a.Length];
        for (int i = 0; i < a.Length; i++) {
            p[i] = new PointF(x + a[i][0] * s, y + a[i][1] * s);
            q[i] = new PointF(p[i].X + 1.1f * s, p[i].Y + 1.5f * s);
        }
        using (Graphics g = Graphics.FromImage(b)) {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            using (var sh = new SolidBrush(Color.FromArgb(64, 0, 0, 0))) g.FillPolygon(sh, q);
            using (var pen = new Pen(Color.FromArgb(255, 26, 26, 26), 1.4f * s) { LineJoin = LineJoin.Round })
            using (var fill = new SolidBrush(Color.White)) {
                g.FillPolygon(fill, p);
                g.DrawPolygon(pen, p);
            }
        }
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
# With -ClientOnly the rectangle is the client area (no title bar, no border).
function Get-WindowShot([IntPtr]$Hwnd, [switch]$ClientOnly) {
    $c = [Cap]::ClientRect($Hwnd)
    $r = if ($ClientOnly) { $c } else { [Cap]::FrameRect($Hwnd) }
    $bmp = [Cap]::Grab($r[0], $r[1], $r[2], $r[3])
    if (-not $ClientOnly) { [Cap]::GrayRing($bmp, 0, 0, $r[2], $r[3]) }
    @{ Rect = $r; Client = $c; Bitmap = $bmp }
}

function Open-Surface([string]$Name) {
    # The showcase variants run on a mock state (EDID profile selected, HDR on) or an in-memory gamut entry;
    # nothing is applied and no file is written.
    $arg = @{ main = 'main-hdr'; gamut = 'gamut-showcase'; profiles = 'profiles-showcase'; traymenu = 'traymenu-showcase' }[$Name]
    & $Exe --debug-open $arg | Out-Null
}

$themes = if ($Theme -eq 'both') { @('light', 'dark') } else { @($Theme) }
New-Item -ItemType Directory -Force -Path $Out, $SiteOut | Out-Null

$classes = @{ main = 'SpectrumMainWindow'; gamut = 'SpectrumGamutWindow'; profiles = 'SpectrumProfilesWindow'; traymenu = '#32768' }

foreach ($t in $themes) {
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

        $surfaces = if ($one) { @('gamut', 'profiles', 'traymenu', 'main') } else { @('gamut', 'profiles', 'main') }
        foreach ($s in $surfaces) {
            Open-Surface $s
            $h = Wait-Window $classes[$s]
            Start-Sleep -Milliseconds 600
            if ($s -ne 'traymenu' -and -not [Cap]::IsTopmost($h)) { throw "$s is not above the backdrop; refusing to capture" }
            $shot = if ($one) { Get-WindowShot $h } else { Get-WindowShot $h -ClientOnly }
            if ($s -eq 'traymenu') {
                Save-Png $shot.Bitmap (Join-Path $Out "traymenu-$t.png")
                Save-Png $shot.Bitmap (Join-Path $Out "traymenu-$t-1x.png")
            } else {
                Save-Png $shot.Bitmap (Join-Path $Out "$s-$t$suffix.png")
            }
            Write-Host ("  {0}: {1}x{2} at {3},{4}" -f $s, $shot.Rect[2], $shot.Rect[3], $shot.Rect[0], $shot.Rect[1])

            if ($s -eq 'main') {
                # HDR section: client rows 232..585 of the 440 x 680 logical layout (Advanced expanded, HDR on), full client width.
                $k = $shot.Client[2] / 440.0
                $cx = $shot.Client[0] - $shot.Rect[0]
                $cy = $shot.Client[1] - $shot.Rect[1]
                $y0 = $cy + [int][Math]::Round(232 * $k)
                $y1 = $cy + [int][Math]::Round(585 * $k)
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
            $stageWnd = Wait-Window 'SpectrumStage'
            $dir = Join-Path $RawOut "demo-$t"
            if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $dir | Out-Null
            $layoutFile = Join-Path $RawOut "demo-$t.layout.txt"
            if (Test-Path $layoutFile) { Remove-Item $layoutFile -Force }

            # The demo measures its own tray menu, then writes the layout: capture region, rectangles, start
            # time (Unix ms), length and the scripted cursor path.
            $demoProc = Start-Process -FilePath $Exe -ArgumentList '--debug-demo', '--theme', $t, '--out', $layoutFile -PassThru
            $sw = [Diagnostics.Stopwatch]::StartNew()
            while (-not (Test-Path $layoutFile)) {
                if ($demoProc.HasExited -or $sw.ElapsedMilliseconds -gt 15000) { throw 'the demo did not write its layout file' }
                Start-Sleep -Milliseconds 40
            }
            $L = @{}; $kt = @(); $kx = @(); $ky = @()
            foreach ($line in Get-Content $layoutFile) {
                $p = $line -split ' '
                if ($p[0] -eq 'key') { $kt += [double]$p[1]; $kx += [double]$p[2]; $ky += [double]$p[3] }
                else { $L[$p[0]] = [double[]]($p[1..($p.Length - 1)]) }
            }
            $reg = [int[]]$L['region']; $m = [int[]]$L['main']; $q = [int[]]$L['menu']; $bar = [int[]]$L['bar']
            $x0 = $reg[0]; $y0 = $reg[1]; $w = $reg[2]; $h = $reg[3]
            $t0 = [long]$L['t0'][0]; $end = [long]$L['end'][0]; $cscale = [single]$L['scale'][0]
            # The region must lie inside the stage, so no desktop, taskbar or other window can be in it.
            $sr = [Cap]::FrameRect($stageWnd)
            if ($x0 -lt $sr[0] -or $y0 -lt $sr[1] -or ($x0 + $w) -gt ($sr[0] + $sr[2]) -or ($y0 + $h) -gt ($sr[1] + $sr[3])) {
                throw 'the demo region is not inside the stage'
            }
            Wait-Window 'SpectrumMainWindow' | Out-Null
            while (([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()) -lt $t0) { Start-Sleep -Milliseconds 2 }

            $frames = New-Object System.Collections.Generic.List[System.Drawing.Bitmap]
            $times = New-Object System.Collections.Generic.List[double]
            $next = 0
            while ($true) {
                $tn = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0
                if ($tn -ge $end) { break }
                if ($demoProc.HasExited) { throw 'the demo ended before the recording was complete' }
                if ($tn -ge $next) {
                    $frames.Add([Cap]::Grab($x0, $y0, $w, $h))
                    $times.Add($tn + 8)
                    $next += 100
                } else {
                    Start-Sleep -Milliseconds 3
                }
            }
            $demoProc.WaitForExit(6000) | Out-Null

            # Checks: no blank frame, and outside the window, the menu (both with their shadows) and the mock
            # taskbar only the stage color may show.
            $br, $bg, $bb = if ($t -eq 'dark') { 0x20, 0x20, 0x20 } else { 0xE9, 0xE6, 0xE1 }
            $keep = @(
                [int[]]@(($m[0] - $x0 - 80), ($m[1] - $y0 - 80), ($m[2] + 160), ($m[3] + 160)),
                [int[]]@(($q[0] - $x0 - 80), ($q[1] - $y0 - 80), ($q[2] + 160), ($q[3] + 160)),
                [int[]]@(($bar[0] - $x0 - 1), ($bar[1] - $y0 - 1), ($bar[2] + 2), ($bar[3] + 2)))
            $blank = 0; $bad = 0
            foreach ($f in $frames) {
                if ([Cap]::IsBlank($f, $br, $bg, $bb)) { $blank++ }
                if ([Cap]::Stray($f, $br, $bg, $bb, [int[][]]$keep) -gt 0) { $bad++ }
            }
            if ($blank -gt 0 -or $bad -gt 0 -or $frames.Count -lt 10) {
                if ($frames.Count -gt 0) { Save-Png $frames[[int]($frames.Count / 2)] (Join-Path $RawOut 'failed-frame.png') }
                throw "$blank blank and $bad stray-pixel frames of $($frames.Count); frames not saved (see failed-frame.png)"
            }

            # Cursor sprite at the scripted position, interpolated at each frame's time.
            $i = 1
            for ($n = 0; $n -lt $frames.Count; $n++) {
                $f = $frames[$n]
                $k = [Math]::Min([Math]::Max($times[$n] / 20.0, 0), $kt.Count - 1)
                $k0 = [int][Math]::Floor($k); $k1 = [Math]::Min($k0 + 1, $kt.Count - 1); $fr = $k - $k0
                $cx = $kx[$k0] + ($kx[$k1] - $kx[$k0]) * $fr - $x0
                $cy = $ky[$k0] + ($ky[$k1] - $ky[$k0]) * $fr - $y0
                [Cap]::GrayRing($f, $m[0] - $x0, $m[1] - $y0, $m[2], $m[3])
                [Cap]::Sprite($f, [single]$cx, [single]$cy, $cscale)
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
