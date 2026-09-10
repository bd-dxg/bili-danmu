# Edge TTS 协议探测脚本
#
# 目的：在写 Rust 之前先验证「自研直连」这条链路能通、能拿到可播放的音频。
# 用法：
#   .\scripts\edge-tts-probe.ps1 -Text "你好世界" -Out "$env:TEMP\edge-tts-probe.mp3"
#   .\scripts\edge-tts-probe.ps1 -OutputFormat raw-16khz-16bit-mono-pcm -Out "$env:TEMP\pcm.bin"
#
# 参考实现：rany2/edge-tts（Python）；协议细节见 Task/findings.md

param(
    [string]$Text = "你好，欢迎来到直播间",
    [string]$Voice = "zh-CN-XiaoxiaoNeural",
    [string]$Rate = "+0%",
    [string]$Pitch = "+0Hz",
    [string]$Volume = "+0%",
    [string]$OutputFormat = "audio-24khz-48kbitrate-mono-mp3",
    [string]$Out = "$env:TEMP\edge-tts-probe.audio",
    [switch]$DumpHeader,
    [switch]$Quiet
)

$ErrorActionPreference = "Stop"

$TRUSTED_CLIENT_TOKEN = "6A5AA1D4EAFF4E9FB37E23D68491D6F4"
$SEC_MS_GEC_VERSION = "1-143.0.3650.75"
$WIN_EPOCH = 11644473600

function New-SecMsGec {
    $unix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $ticks = [int64]($unix + $WIN_EPOCH)
    $ticks = $ticks - ($ticks % 300)          # 向下取整到 5 分钟
    $ticks = $ticks * 10000000                # 转 100ns 单位
    $raw = "$ticks$TRUSTED_CLIENT_TOKEN"
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha.ComputeHash([Text.Encoding]::ASCII.GetBytes($raw))
    } finally {
        $sha.Dispose()
    }
    # 大写 hex
    return (($hash | ForEach-Object { $_.ToString("X2") }) -join "")
}

function New-DateString {
    $culture = [Globalization.CultureInfo]::InvariantCulture
    return [DateTime]::UtcNow.ToString("ddd MMM dd yyyy HH:mm:ss 'GMT+0000 (Coordinated Universal Time)'", $culture)
}

function New-Hex([int]$Bytes) {
    $buf = New-Object byte[] $Bytes
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($buf)
    return (($buf | ForEach-Object { $_.ToString("X2") }) -join "")
}

function Send-Text($ws, [string]$payload) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($payload)
    $ws.SendAsync(
        [ArraySegment[byte]]::new($bytes),
        [System.Net.WebSockets.WebSocketMessageType]::Text,
        $true,
        [Threading.CancellationToken]::None
    ).GetAwaiter().GetResult() | Out-Null
}

$url = "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1" +
"`?TrustedClientToken=$TRUSTED_CLIENT_TOKEN" +
"&ConnectionId=$(New-Hex 16)" +
"&Sec-MS-GEC=$(New-SecMsGec)" +
"&Sec-MS-GEC-Version=$SEC_MS_GEC_VERSION"

if (-not $Quiet) { Write-Host "→ 连接 $url" }

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader("Origin", "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold")
$ws.Options.SetRequestHeader("Pragma", "no-cache")
$ws.Options.SetRequestHeader("Cache-Control", "no-cache")
$ws.Options.SetRequestHeader("Cookie", "muid=$(New-Hex 16);")
$ws.Options.SetRequestHeader(
    "User-Agent",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0"
)

$cts = [Threading.CancellationTokenSource]::new()
$cts.CancelAfter([TimeSpan]::FromSeconds(30))

try {
    $swConnect = [Diagnostics.Stopwatch]::StartNew()
    $ws.ConnectAsync([Uri]$url, $cts.Token).GetAwaiter().GetResult()
    $swConnect.Stop()
    $connectMs = $swConnect.ElapsedMilliseconds
} catch {
    Write-Host "✗ 握手失败：$($_.Exception.Message)" -ForegroundColor Red
    Write-Host "  403 = 被限流或需要 MUID/DRM 令牌；也可能是网络不可达。"
    exit 1
}

if (-not $Quiet) { Write-Host "✓ 握手成功，发送 speech.config + ssml" }

$now = New-DateString
$json = '{"context":{"synthesis":{"audio":{"metadataoptions":{' +
'"sentenceBoundaryEnabled":"false","wordBoundaryEnabled":"false"},' +
'"outputFormat":"' + $OutputFormat + '"}}}}'
Send-Text $ws ("X-Timestamp:$now`r`n" +
"Content-Type:application/json; charset=utf-8`r`n" +
"Path:speech.config`r`n`r`n" +
$json + "`r`n")

$escaped = $Text.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;")
$ssml = "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'>" +
"<voice name='$Voice'>" +
"<prosody pitch='$Pitch' rate='$Rate' volume='$Volume'>$escaped</prosody>" +
"</voice></speak>"
Send-Text $ws ("X-RequestId:$(New-Hex 16)`r`n" +
"Content-Type:application/ssml+xml`r`n" +
"X-Timestamp:$now" + "Z`r`n" +
"Path:ssml`r`n`r`n" +
$ssml)

$audio = [System.IO.MemoryStream]::new()
$buf = New-Object byte[] 65536
$done = $false
$binCount = 0
$textPaths = [System.Collections.Generic.List[string]]::new()
# 延迟拆解：握手 / 首字节（= 服务端开始出声）/ 完整收流
$firstByteMs = -1
$swShot = [Diagnostics.Stopwatch]::StartNew()

while (-not $done) {
    $seg = [ArraySegment[byte]]::new($buf)
    $res = $ws.ReceiveAsync($seg, $cts.Token).GetAwaiter().GetResult()
    if ($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
        Write-Host "✗ 服务端提前关闭连接（可能限流或参数不合法）" -ForegroundColor Red
        break
    }
    $chunk = $buf[0..($res.Count - 1)]

    if ($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Text) {
        $text = [Text.Encoding]::UTF8.GetString($chunk)
        $sep = $text.IndexOf("`r`n`r`n")
        $header = if ($sep -ge 0) { $text.Substring(0, $sep) } else { $text }
        $path = ($header -split "`r`n" | Where-Object { $_ -like "Path:*" }) -replace "^Path:", ""
        $textPaths.Add($path)
        if ($path -eq "turn.end") { $done = $true }
    } else {
        # 二进制：前 2 字节大端 = header 长度
        if ($chunk.Length -lt 2) { continue }
        $headerLen = ($chunk[0] -shl 8) -bor $chunk[1]
        if ($DumpHeader -and $binCount -eq 0) {
            Write-Host "[调试] 帧总长=$($chunk.Length) 长度字段=$headerLen"
            Write-Host "[调试] data[0..$headerLen) 前 120 字符："
            Write-Host ([Text.Encoding]::ASCII.GetString($chunk, 0, [Math]::Min(120, $chunk.Length)) -replace "`r", "\r" -replace "`n", "\n")
            Write-Host "[调试] data[2..$($headerLen + 2)) 前 120 字符："
            Write-Host ([Text.Encoding]::ASCII.GetString($chunk, 2, [Math]::Min(120, $chunk.Length - 2)) -replace "`r", "\r" -replace "`n", "\n")
        }
        $binCount++
        $bodyStart = $headerLen + 2
        if ($firstByteMs -lt 0) { $firstByteMs = $swShot.ElapsedMilliseconds }
        if ($chunk.Length -le $bodyStart) { continue }
        $audio.Write($chunk, $bodyStart, $chunk.Length - $bodyStart)
    }
}

$swShot.Stop()
$totalMs = $swShot.ElapsedMilliseconds

$ws.Dispose()
$bytes = $audio.ToArray()
$audio.Dispose()

if (-not $Quiet) {
    Write-Host "  文本帧 Path 序列：$([string]::Join(", ", $textPaths))"
    # 延迟拆解：握手是固定开销（每条弹幕都要重付一次），首字节才是用户“听到声音”的时刻
    Write-Host ("  耗时：握手 {0}ms + 合成首字节 {1}ms = 首音 {2}ms；完整收流 {3}ms" -f `
        $connectMs, $firstByteMs, ($connectMs + $firstByteMs), ($connectMs + $totalMs))
}

if ($bytes.Length -eq 0) {
    Write-Host "✗ 没有收到音频数据（outputFormat=$OutputFormat 可能不被支持）" -ForegroundColor Red
    exit 1
}

[System.IO.File]::WriteAllBytes($Out, $bytes)
Write-Host "✓ 收到音频 $($bytes.Length) 字节 → $Out" -ForegroundColor Green
Write-Host "  前 4 字节（MP3 应以 FF F3/FB 开头）：$((($bytes[0..3]) | ForEach-Object { $_.ToString('X2') }) -join ' ')"
