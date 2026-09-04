# B站弹幕 WS 协议探测脚本（诊断用）
# 独立连接房间，统计 25 秒内收到的操作码/命令分布，判断是否推送弹幕
param([int]$RoomId = 22907643, [int]$DurationSec = 10, [int]$ProtoVer = 3, [string]$Platform = "web", [long]$Uid = 0, [string]$Buvid = "")

$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8

$ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

# 1. 获取 token（getConf 无风控）
$conf = Invoke-RestMethod -Uri "https://api.live.bilibili.com/room/v1/Danmu/getConf?room_id=$RoomId&platform=pc&player=web" -Headers @{ "User-Agent" = $ua }
if ($conf.code -ne 0) { Write-Output "getConf failed: $($conf.message)"; exit 1 }
$token = $conf.data.token
$host1 = $conf.data.host_server_list[0]
Write-Output "token ok, server: $($host1.host):$($host1.wss_port)"

# 2. 连接 WSS
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader("User-Agent", $ua)
$ws.Options.SetRequestHeader("Origin", "https://live.bilibili.com")
$uri = "wss://$($host1.host):$($host1.wss_port)/sub"
$ws.ConnectAsync([Uri]$uri, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
Write-Output "connected: $uri"

function Send-Packet([uint32]$op, [byte[]]$body) {
    $len = [BitConverter]::GetBytes([uint32](16 + $body.Length))  # LE! 需要反转
    [Array]::Reverse($len)
    $buf = New-Object byte[] (16 + $body.Length)
    $len.CopyTo($buf, 0)
    $h16 = [BitConverter]::GetBytes([uint16]16); [Array]::Reverse($h16); $h16.CopyTo($buf, 4)
    $p0 = [BitConverter]::GetBytes([uint16]0); [Array]::Reverse($p0); $p0.CopyTo($buf, 6)
    $opb = [BitConverter]::GetBytes($op); [Array]::Reverse($opb); $opb.CopyTo($buf, 8)
    $s1 = [BitConverter]::GetBytes([uint32]1); [Array]::Reverse($s1); $s1.CopyTo($buf, 12)
    $body.CopyTo($buf, 16)
    $ws.SendAsync([ArraySegment[byte]]::new($buf), [System.Net.WebSockets.WebSocketMessageType]::Binary, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
}

function Read-BigU32([byte[]]$b, [int]$off) { return ([uint32]$b[$off] -shl 24) -bor ([uint32]$b[$off+1] -shl 16) -bor ([uint32]$b[$off+2] -shl 8) -bor $b[$off+3] }
function Read-BigU16([byte[]]$b, [int]$off) { return ([uint16]$b[$off] -shl 8) -bor $b[$off+1] }

function Inflate-Brotli([byte[]]$data) {
    $ms = [System.IO.MemoryStream]::new($data)
    $bs = [System.IO.Compression.BrotliStream]::new($ms, [System.IO.Compression.CompressionMode]::Decompress)
    $out = [System.IO.MemoryStream]::new()
    $bs.CopyTo($out)
    $bs.Dispose()
    return $out.ToArray()
}

function Inflate-Zlib([byte[]]$data) {
    $ms = [System.IO.MemoryStream]::new($data)
    $zs = [System.IO.Compression.ZLibStream]::new($ms, [System.IO.Compression.CompressionMode]::Decompress)
    $out = [System.IO.MemoryStream]::new()
    $zs.CopyTo($out)
    $zs.Dispose()
    return $out.ToArray()
}

function Parse-Cmds([byte[]]$data) {
    # data 是展开后的 op5 命令字节（可能含多个 16B 包）
    $results = New-Object System.Collections.Generic.List[string]
    $off = 0
    while ($off + 16 -le $data.Length) {
        $total = Read-BigU32 $data $off
        if ($total -lt 16 -or ($off + $total) -gt $data.Length) { break }
        $proto = Read-BigU16 $data ($off + 6)
        $op = Read-BigU32 $data ($off + 8)
        $body = $data[($off+16)..($off+$total-1)]
        if ($op -eq 5) {
            if ($proto -eq 0) {
                $results.Add([Text.Encoding]::UTF8.GetString($body))
            } elseif ($proto -eq 3) {
                $inner = Inflate-Brotli $body
                foreach ($c in (Parse-Cmds $inner)) { $results.Add($c) }
            } elseif ($proto -eq 2) {
                $inner = Inflate-Zlib $body
                foreach ($c in (Parse-Cmds $inner)) { $results.Add($c) }
            }
        }
        $off += $total
    }
    return $results
}

# 3. 认证
$authBody = [Text.Encoding]::UTF8.GetBytes(("{0}" -f (@{ uid = $Uid; roomid = $RoomId; protover = $ProtoVer; platform = $Platform; type = 2; key = $token; if ($Buvid) { # B站弹幕 WS 协议探测脚本（诊断用）
# 独立连接房间，统计 25 秒内收到的操作码/命令分布，判断是否推送弹幕
param([int]$RoomId = 22907643, [int]$DurationSec = 10, [int]$ProtoVer = 3, [string]$Platform = "web", [long]$Uid = 0, [string]$Buvid = "")

$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8

$ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

# 1. 获取 token（getConf 无风控）
$conf = Invoke-RestMethod -Uri "https://api.live.bilibili.com/room/v1/Danmu/getConf?room_id=$RoomId&platform=pc&player=web" -Headers @{ "User-Agent" = $ua }
if ($conf.code -ne 0) { Write-Output "getConf failed: $($conf.message)"; exit 1 }
$token = $conf.data.token
$host1 = $conf.data.host_server_list[0]
Write-Output "token ok, server: $($host1.host):$($host1.wss_port)"

# 2. 连接 WSS
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader("User-Agent", $ua)
$ws.Options.SetRequestHeader("Origin", "https://live.bilibili.com")
$uri = "wss://$($host1.host):$($host1.wss_port)/sub"
$ws.ConnectAsync([Uri]$uri, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
Write-Output "connected: $uri"

function Send-Packet([uint32]$op, [byte[]]$body) {
    $len = [BitConverter]::GetBytes([uint32](16 + $body.Length))  # LE! 需要反转
    [Array]::Reverse($len)
    $buf = New-Object byte[] (16 + $body.Length)
    $len.CopyTo($buf, 0)
    $h16 = [BitConverter]::GetBytes([uint16]16); [Array]::Reverse($h16); $h16.CopyTo($buf, 4)
    $p0 = [BitConverter]::GetBytes([uint16]0); [Array]::Reverse($p0); $p0.CopyTo($buf, 6)
    $opb = [BitConverter]::GetBytes($op); [Array]::Reverse($opb); $opb.CopyTo($buf, 8)
    $s1 = [BitConverter]::GetBytes([uint32]1); [Array]::Reverse($s1); $s1.CopyTo($buf, 12)
    $body.CopyTo($buf, 16)
    $ws.SendAsync([ArraySegment[byte]]::new($buf), [System.Net.WebSockets.WebSocketMessageType]::Binary, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
}

function Read-BigU32([byte[]]$b, [int]$off) { return ([uint32]$b[$off] -shl 24) -bor ([uint32]$b[$off+1] -shl 16) -bor ([uint32]$b[$off+2] -shl 8) -bor $b[$off+3] }
function Read-BigU16([byte[]]$b, [int]$off) { return ([uint16]$b[$off] -shl 8) -bor $b[$off+1] }

function Inflate-Brotli([byte[]]$data) {
    $ms = [System.IO.MemoryStream]::new($data)
    $bs = [System.IO.Compression.BrotliStream]::new($ms, [System.IO.Compression.CompressionMode]::Decompress)
    $out = [System.IO.MemoryStream]::new()
    $bs.CopyTo($out)
    $bs.Dispose()
    return $out.ToArray()
}

function Inflate-Zlib([byte[]]$data) {
    $ms = [System.IO.MemoryStream]::new($data)
    $zs = [System.IO.Compression.ZLibStream]::new($ms, [System.IO.Compression.CompressionMode]::Decompress)
    $out = [System.IO.MemoryStream]::new()
    $zs.CopyTo($out)
    $zs.Dispose()
    return $out.ToArray()
}

function Parse-Cmds([byte[]]$data) {
    # data 是展开后的 op5 命令字节（可能含多个 16B 包）
    $results = New-Object System.Collections.Generic.List[string]
    $off = 0
    while ($off + 16 -le $data.Length) {
        $total = Read-BigU32 $data $off
        if ($total -lt 16 -or ($off + $total) -gt $data.Length) { break }
        $proto = Read-BigU16 $data ($off + 6)
        $op = Read-BigU32 $data ($off + 8)
        $body = $data[($off+16)..($off+$total-1)]
        if ($op -eq 5) {
            if ($proto -eq 0) {
                $results.Add([Text.Encoding]::UTF8.GetString($body))
            } elseif ($proto -eq 3) {
                $inner = Inflate-Brotli $body
                foreach ($c in (Parse-Cmds $inner)) { $results.Add($c) }
            } elseif ($proto -eq 2) {
                $inner = Inflate-Zlib $body
                foreach ($c in (Parse-Cmds $inner)) { $results.Add($c) }
            }
        }
        $off += $total
    }
    return $results
}

# 3. 认证
$authBody = [Text.Encoding]::UTF8.GetBytes(("{0}" -f (@{ uid = $Uid; roomid = $RoomId; protover = $ProtoVer; platform = $Platform; type = 2; key = $token } | ConvertTo-Json -Compress)))
Send-Packet 7 $authBody
Write-Output "auth sent protover=$ProtoVer platform=$Platform"

# 4. receive: 累积到整帧(EndOfMessage)再解析
$stats = @{}
$danmuSamples = New-Object System.Collections.Generic.List[string]
$start = [DateTime]::Now
$buf = New-Object byte[] (4 * 1024 * 1024)
$pending = New-Object System.Collections.Generic.List[byte]
while (([DateTime]::Now - $start).TotalSeconds -lt $DurationSec) {
    $seg = [ArraySegment[byte]]::new($buf)
    $res = $ws.ReceiveAsync($seg, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    if ($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) { Write-Output "server closed"; break }
    for ($i = 0; $i -lt $res.Count; $i++) { $pending.Add($buf[$i]) }
    if (-not $res.EndOfMessage) { continue }
    $recv = $pending.ToArray()
    $pending.Clear()
    $off = 0
    while ($off + 16 -le $recv.Length) {
        $total = Read-BigU32 $recv $off
        if ($total -lt 16 -or ($off + $total) -gt $recv.Length) { break }
        $proto = Read-BigU16 $recv ($off + 6)
        $op = Read-BigU32 $recv ($off + 8)
        $key = "op=$op"
        if (-not $stats.ContainsKey($key)) { $stats[$key] = 0 }
        $stats[$key]++
        if ($op -eq 5) {
            $cmds = Parse-Cmds ([byte[]]@($recv[$off..($off+$total-1)]))
            if ($cmds) {
                foreach ($c in $cmds) {
                    $cmdName = ""
                    try { $j = $c | ConvertFrom-Json; $cmdName = $j.cmd } catch { $cmdName = "(bad json)" }
                    $k2 = "cmd:$cmdName"
                    if (-not $stats.ContainsKey($k2)) { $stats[$k2] = 0 }
                    $stats[$k2]++
                    if ($cmdName -eq "DANMU_MSG" -and $danmuSamples.Count -lt 3) {
                        $danmuSamples.Add($c.Substring(0, [Math]::Min(4000, $c.Length)))
                    }
                }
            }
        }
        $off += $total
    }
}
$ws.Dispose()
Write-Output "=== STATS ($DurationSec sec) ==="
$stats.GetEnumerator() | Sort-Object Name | ForEach-Object { Write-Output "$($_.Name): $($_.Value)" }
Write-Output "=== DANMU_MSG SAMPLES ==="
if ($danmuSamples.Count -eq 0) { Write-Output "(none)" } else { $danmuSamples | ForEach-Object { Write-Output $_ } }





.buvid = $Buvid } } | ConvertTo-Json -Compress)))
Send-Packet 7 $authBody
Write-Output "auth sent protover=$ProtoVer platform=$Platform"

# 4. receive: 累积到整帧(EndOfMessage)再解析
$stats = @{}
$danmuSamples = New-Object System.Collections.Generic.List[string]
$start = [DateTime]::Now
$buf = New-Object byte[] (4 * 1024 * 1024)
$pending = New-Object System.Collections.Generic.List[byte]
while (([DateTime]::Now - $start).TotalSeconds -lt $DurationSec) {
    $seg = [ArraySegment[byte]]::new($buf)
    $res = $ws.ReceiveAsync($seg, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    if ($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) { Write-Output "server closed"; break }
    for ($i = 0; $i -lt $res.Count; $i++) { $pending.Add($buf[$i]) }
    if (-not $res.EndOfMessage) { continue }
    $recv = $pending.ToArray()
    $pending.Clear()
    $off = 0
    while ($off + 16 -le $recv.Length) {
        $total = Read-BigU32 $recv $off
        if ($total -lt 16 -or ($off + $total) -gt $recv.Length) { break }
        $proto = Read-BigU16 $recv ($off + 6)
        $op = Read-BigU32 $recv ($off + 8)
        $key = "op=$op"
        if (-not $stats.ContainsKey($key)) { $stats[$key] = 0 }
        $stats[$key]++
        if ($op -eq 5) {
            $cmds = Parse-Cmds ([byte[]]@($recv[$off..($off+$total-1)]))
            if ($cmds) {
                foreach ($c in $cmds) {
                    $cmdName = ""
                    try { $j = $c | ConvertFrom-Json; $cmdName = $j.cmd } catch { $cmdName = "(bad json)" }
                    $k2 = "cmd:$cmdName"
                    if (-not $stats.ContainsKey($k2)) { $stats[$k2] = 0 }
                    $stats[$k2]++
                    if ($cmdName -eq "DANMU_MSG" -and $danmuSamples.Count -lt 3) {
                        $danmuSamples.Add($c.Substring(0, [Math]::Min(4000, $c.Length)))
                    }
                }
            }
        }
        $off += $total
    }
}
$ws.Dispose()
Write-Output "=== STATS ($DurationSec sec) ==="
$stats.GetEnumerator() | Sort-Object Name | ForEach-Object { Write-Output "$($_.Name): $($_.Value)" }
Write-Output "=== DANMU_MSG SAMPLES ==="
if ($danmuSamples.Count -eq 0) { Write-Output "(none)" } else { $danmuSamples | ForEach-Object { Write-Output $_ } }






