# B站弹幕 WS 协议探测脚本（诊断用）
# 独立连接房间，统计一段时间内收到的操作码/命令分布，判断推送了哪些事件
#
# 登录态：不加 -UseConfigCookie 走游客 getConf（B 站 2025+ 不向游客推房间事件，
# 只能收到全网广播），要看真实事件必须带登录 Cookie——此时改走
# getDanmuInfo（WBI 签名）拿登录级 token，与 app 的连接路径一致。
param(
    [int]$RoomId = 22907643,
    [int]$DurationSec = 10,
    [int]$ProtoVer = 3,
    [string]$Platform = "web",
    [long]$Uid = 0,
    [string]$Buvid = "",
    # 读 %APPDATA%\com.bilidanmu.app\config.json 里的登录 Cookie（DPAPI 解密，不用手抄）
    [switch]$UseConfigCookie,
    # 已解密的 Cookie 串（不想用 -UseConfigCookie 时可显式传入）
    [string]$Cookie = "",
    # dump 指定 cmd 的原始 JSON（如 INTERACT_WORD），配合 -DumpCount
    [string]$DumpCmd = "",
    [int]$DumpCount = 2,
    # 每种 cmd 各 dump 第一条：还不知道有哪些 cmd 时用，一次拿到清单与字段
    [switch]$DumpFirst
)

$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8

$ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

# ---- WBI 签名（与 src-tauri/src/bilibili/wbi.rs 同构，改一处两边都得改）----
# 固定重排映射表（64 项，全站统一）
$MIXIN_KEY_ENC_TAB = @(46,47,18,2,53,8,23,32,15,50,10,31,58,3,45,35,27,43,5,49,33,9,42,19,29,28,14,39,12,38,41,13,37,48,7,16,24,55,40,61,26,17,0,1,60,51,30,4,22,25,54,21,56,59,6,63,57,62,11,36,20,34,44,52)

function Get-MixinKey([string]$imgKey, [string]$subKey) {
    $chars = "$imgKey$subKey".ToCharArray()
    if ($chars.Length -lt 64) { throw "wbi key 长度异常（img=$imgKey sub=$subKey），可能接口已改版" }
    return (-join (0..31 | ForEach-Object { $chars[$MIXIN_KEY_ENC_TAB[$_]] }))
}

function New-WbiQuery([hashtable]$params, [string]$mixin) {
    $params['wts'] = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $query = (($params.Keys | Sort-Object | ForEach-Object { "$_=$([Uri]::EscapeDataString([string]$params[$_]))" }) -join '&')
    # 过滤 !'()*（WBI 规范）。本脚本的参数只有 id/type/wts，全是数字，EscapeDataString
    # 与 encodeURIComponent 在这个字符集上没有差异，不会踩到两者转义规则的不同
    $filtered = $query -replace "[!'()*]", ''
    $md5 = [System.Security.Cryptography.MD5]::Create()
    $rid = -join ($md5.ComputeHash([Text.Encoding]::UTF8.GetBytes("$filtered$mixin")) | ForEach-Object { $_.ToString('x2') })
    return "$filtered&w_rid=$rid"
}

# 从本机 app 配置读登录 Cookie（dpapi: 前缀 → DPAPI 解密，绑定当前 Windows 用户；旧版明文直接兼容）
function Get-LoginFromConfig {
    $p = Join-Path $env:APPDATA 'com.bilidanmu.app\config.json'
    if (-not (Test-Path $p)) { throw "配置文件不存在：$p（先在 app 里扫码登录）" }
    $cfg = Get-Content $p -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not $cfg.auth) { throw "配置里没有登录态（先在 app 里扫码登录）" }
    $stored = $cfg.auth.cookies
    $plain = $stored
    if ($stored.StartsWith('dpapi:')) {
        $hex = $stored.Substring(6)
        $blob = New-Object byte[] ($hex.Length / 2)
        for ($i = 0; $i -lt $blob.Length; $i++) { $blob[$i] = [Convert]::ToByte($hex.Substring($i * 2, 2), 16) }
        $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($blob, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
        $plain = [Text.Encoding]::UTF8.GetString($dec)
    }
    return @{ Cookie = $plain; Uid = [long]$cfg.auth.uid }
}

# 1. 获取 token：登录态走 getDanmuInfo（WBI 签名），否则回退游客 getConf
if ($UseConfigCookie -or $Cookie) {
    if ($Cookie) {
        $cookies = $Cookie
    } else {
        $login = Get-LoginFromConfig
        $cookies = $login.Cookie
        if ($Uid -eq 0) { $Uid = $login.Uid }
    }
    $nav = Invoke-RestMethod -Uri "https://api.bilibili.com/x/web-interface/nav" -Headers @{ "User-Agent" = $ua; "Cookie" = $cookies }
    $imgKey = ($nav.data.wbi_img.img_url -split '/')[-1] -replace '\.png$', ''
    $subKey = ($nav.data.wbi_img.sub_url -split '/')[-1] -replace '\.png$', ''
    $q = New-WbiQuery @{ id = "$RoomId"; type = '0' } (Get-MixinKey $imgKey $subKey)
    $info = Invoke-RestMethod -Uri "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?$q" -Headers @{ "User-Agent" = $ua; "Cookie" = $cookies }
    if ($info.code -ne 0) { Write-Output "getDanmuInfo failed code=$($info.code) message=$($info.message)"; exit 1 }
    $token = $info.data.token
    $host1 = $info.data.host_list[0]
    Write-Output "登录态 token ok (uid=$Uid), server: $($host1.host):$($host1.wss_port)"
} else {
    Write-Output "未带登录 Cookie：走游客 getConf（B 站 2025+ 不推房间事件，多半只能收到广播）"
    $conf = Invoke-RestMethod -Uri "https://api.live.bilibili.com/room/v1/Danmu/getConf?room_id=$RoomId&platform=pc&player=web" -Headers @{ "User-Agent" = $ua }
    if ($conf.code -ne 0) { Write-Output "getConf failed: $($conf.message)"; exit 1 }
    $token = $conf.data.token
    $host1 = $conf.data.host_server_list[0]
    Write-Output "游客 token ok, server: $($host1.host):$($host1.wss_port)"
}

# 2. 连接 WSS
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader("User-Agent", $ua)
$ws.Options.SetRequestHeader("Origin", "https://live.bilibili.com")
$uri = "wss://$($host1.host):$($host1.wss_port)/sub"
[void]$ws.ConnectAsync([Uri]$uri, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
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
    [void]$ws.SendAsync([ArraySegment[byte]]::new($buf), [System.Net.WebSockets.WebSocketMessageType]::Binary, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
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
$auth = @{ uid = $Uid; roomid = $RoomId; protover = $ProtoVer; platform = $Platform; type = 2; key = $token }
# 可选带上 buvid（部分房间缺它会被当游客处理）
if ($Buvid) { $auth.buvid = $Buvid }
$authBody = [Text.Encoding]::UTF8.GetBytes(("{0}" -f ($auth | ConvertTo-Json -Compress)))
Send-Packet 7 $authBody
Write-Output "auth sent protover=$ProtoVer platform=$Platform"

# 4. receive: 累积到整帧(EndOfMessage)再解析
$stats = @{}
$danmuSamples = New-Object System.Collections.Generic.List[string]
$dumpSamples = New-Object System.Collections.Generic.List[string]
$dumpedCmds = @{}
$start = [DateTime]::Now
$buf = New-Object byte[] (4 * 1024 * 1024)
$pending = New-Object System.Collections.Generic.List[byte]
// B 站要求 30 秒内至少一次心跳（op=2），不发服务端会主动断开——
// 之前的表现就是「抓不到几十秒连接就没了」，长窗口采集根本跑不完，
// 而统计标签还写着 $DurationSec，数据量被高估。
// ClientWebSocket 允许收 / 发并发，所以 ReceiveAsync 挂在后台等，主线程按拍发心跳。
$HEARTBEAT_SECS = 25
$lastBeat = [DateTime]::Now
while (([DateTime]::Now - $start).TotalSeconds -lt $DurationSec) {
    $seg = [ArraySegment[byte]]::new($buf)
    $recvTask = $ws.ReceiveAsync($seg, [Threading.CancellationToken]::None)
    # 等收包期间按拍发心跳（不能阻塞在 GetResult 上，否则心跳发不出去）
    while (-not $recvTask.IsCompleted) {
        if (([DateTime]::Now - $start).TotalSeconds -ge $DurationSec) { break }
        if (([DateTime]::Now - $lastBeat).TotalSeconds -ge $HEARTBEAT_SECS) {
            Send-Packet 2 ([Text.Encoding]::UTF8.GetBytes("[object Object]"))
            $lastBeat = [DateTime]::Now
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not $recvTask.IsCompleted) { break } // 到点了，收尾
    # 服务端仍可能在任意时刻断开（游客 token 实测几秒就被断），抛异常即视为本次采集结束，
    # 仍要把已统计到的东西打出来
    try {
        $res = $recvTask.GetAwaiter().GetResult()
    } catch {
        Write-Output "连接中断（服务端关闭）：$($_.Exception.InnerException.Message)"
        break
    }
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
                    if ($DumpCmd -and $cmdName -eq $DumpCmd -and $dumpSamples.Count -lt $DumpCount) {
                        $dumpSamples.Add($c)
                    }
                    if ($DumpFirst -and -not $dumpedCmds.ContainsKey($k2)) {
                        $dumpedCmds[$k2] = $true
                        $dumpSamples.Add("[FIRST:$cmdName] " + $c.Substring(0, [Math]::Min(4000, $c.Length)))
                    }
                }
            }
        }
        $off += $total
    }
}
if ($recvTask -and -not $recvTask.IsCompleted) { $ws.Abort() }
$ws.Dispose()
Write-Output "=== STATS (目标 $DurationSec 秒，实际 $([int]([DateTime]::Now - $start).TotalSeconds) 秒) ==="
$stats.GetEnumerator() | Sort-Object Name | ForEach-Object { Write-Output "$($_.Name): $($_.Value)" }
Write-Output "=== DANMU_MSG SAMPLES ==="
if ($danmuSamples.Count -eq 0) { Write-Output "(none)" } else { $danmuSamples | ForEach-Object { Write-Output $_ } }
if ($DumpCmd -or $DumpFirst) {
    $label = if ($DumpFirst) { 'first of each cmd' } else { $DumpCmd }
    Write-Output "=== DUMP ($label) ==="
    if ($dumpSamples.Count -eq 0) { Write-Output "(none)" } else { $dumpSamples | ForEach-Object { Write-Output $_ } }
}






