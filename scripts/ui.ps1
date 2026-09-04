# bili-danmu UI 自动化辅助脚本（开发调试用）
# 用法:
#   ui.ps1 rect                显示主窗口矩形
#   ui.ps1 fg                  显示前台窗口
#   ui.ps1 shot <out.png>      截取主窗口保存 png
#   ui.ps1 click <relX> <relY> 点击窗口内相对坐标(0,0=客户区左上)
#   ui.ps1 type <text>         向前台窗口发送按键
#   ui.ps1 analyze <png> <x> <y> <w> <h> <hex>  在 png 区域内查找相近颜色，输出首个匹配窗口内坐标
#   ui.ps1 activemain          激活主窗口
param(
    [Parameter(Mandatory = $true, Position = 0)][string]$Cmd,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Args
)

# 目标窗口标题（默认主窗口；overlay 测试时传 -Title 'bili-danmu overlay'）
$script:Title = "bili-danmu"
for ($i = 0; $i -lt $Args.Count; $i++) {
    if ($Args[$i] -eq "-Title" -and ($i + 1) -lt $Args.Count) {
        $script:Title = $Args[$i + 1]
        $Args = $Args[0..($i-1)] + $Args[($i+2)..($Args.Count-1)]
        break
    }
}

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class UINative {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Get-MainWin {
    # 进程可能有多个窗口（主窗口 + overlay），按精确标题查找
    $pid2 = (Get-Process bili-danmu -ErrorAction Stop | Select-Object -First 1).Id
    $script:Found = [IntPtr]::Zero
    $cb = [UINative+EnumProc]{
        param($h, $lp)
        $sb = New-Object System.Text.StringBuilder 256
        [UINative]::GetWindowText($h, $sb, 256) | Out-Null
        if ($sb.ToString() -eq $script:Title) { $script:Found = $h; return $false }
        return $true
    }
    [UINative]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
    if ($script:Found -eq [IntPtr]::Zero) { throw "窗口 '$script:Title' 未找到" }
    return $script:Found
}

function Get-Rect([IntPtr]$h) {
    $r = New-Object UINative+RECT
    [UINative]::GetWindowRect($h, [ref]$r) | Out-Null
    return $r
}

$h = Get-MainWin

switch ($Cmd) {
    "rect" {
        $r = Get-Rect $h
        Write-Output "window L=$($r.L) T=$($r.T) R=$($r.R) B=$($r.B) size=$($r.R-$r.L)x$($r.B-$r.T)"
    }
    "fg" {
        $fg = [UINative]::GetForegroundWindow()
        $fpid = 0; [UINative]::GetWindowThreadProcessId($fg, [ref]$fpid) | Out-Null
        $fp = Get-Process -Id $fpid -ErrorAction SilentlyContinue
        Write-Output "foreground pid=$fpid name=$($fp.ProcessName)"
    }
    "activemain" {
        [UINative]::ShowWindow($h, 9) | Out-Null  # SW_RESTORE
        [UINative]::SetForegroundWindow($h) | Out-Null
        Write-Output "activated bili-danmu"
    }
    "shot" {
        $out = $Args[0]
        $r = Get-Rect $h
        $w = $r.R - $r.L; $ht = $r.B - $r.T
        $bmp = New-Object System.Drawing.Bitmap($w, $ht)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
        $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
        Write-Output "saved $out ($w x $ht)"
    }
    "click" {
        $relX = [int]$Args[0]; $relY = [int]$Args[1]
        $r = Get-Rect $h
        $x = $r.L + $relX; $y = $r.T + $relY
        [UINative]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Milliseconds 150
        [UINative]::SetCursorPos($x, $y) | Out-Null
        Start-Sleep -Milliseconds 150
        [UINative]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
        [UINative]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
        Write-Output "clicked screen($x,$y) window-rel($relX,$relY)"
    }
    "type" {
        $text = $Args[0]
        [UINative]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Milliseconds 200
        [System.Windows.Forms.SendKeys]::SendWait($text)
        Write-Output "typed: $text"
    }
    "analyze" {
        # analyze <png> <winX> <winY> <winW> <winH> <hex 或 r,g,b> [tolerance]
        $png = $Args[0]; $x0 = [int]$Args[1]; $y0 = [int]$Args[2]; $w = [int]$Args[3]; $ht = [int]$Args[4]
        $colorStr = $Args[5]
        $tol = 30; if ($Args.Count -ge 7) { $tol = [int]$Args[6] }
        if ($colorStr -match '^\#?([0-9A-Fa-f]{6})$') {
            $cv = [Convert]::ToInt32($Matches[1], 16)
            $tr = ($cv -shr 16) -band 0xFF; $tg = ($cv -shr 8) -band 0xFF; $tb = $cv -band 0xFF
        } else {
            $parts = $colorStr.Split(',')
            $tr = [int]$parts[0]; $tg = [int]$parts[1]; $tb = [int]$parts[2]
        }
        $bmp = New-Object System.Drawing.Bitmap($png)
        $xEnd = [math]::Min($x0 + $w, $bmp.Width)
        $yEnd = [math]::Min($y0 + $ht, $bmp.Height)
        $found = @()
        for ($y = [math]::Max(0, $y0); $y -lt $yEnd; $y += 2) {
            for ($x = [math]::Max(0, $x0); $x -lt $xEnd; $x += 2) {
                $c = $bmp.GetPixel($x, $y)
                if ([math]::Abs($c.R - $tr) -le $tol -and [math]::Abs($c.G - $tg) -le $tol -and [math]::Abs($c.B - $tb) -le $tol) {
                    $found += "$($x),$($y)"
                    if ($found.Count -ge 5) { break }
                }
            }
            if ($found.Count -ge 5) { break }
        }
        $bmp.Dispose()
        if ($found.Count -gt 0) { Write-Output "match count>=5 at: $($found -join '; ')" }
        else { Write-Output "no match for #($tr,$tg,$tb) tol=$tol in ($x0,$y0) $w x $ht" }
    }
    default { Write-Output "unknown cmd: $Cmd"; exit 1 }
}
