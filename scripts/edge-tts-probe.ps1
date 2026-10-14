# Edge TTS 协议探测脚本
#
# 目的：在写 Rust 之前先验证「自研直连」这条链路能通、能拿到可播放的音频。
# 用法：
#   .\scripts\edge-tts-probe.ps1 -Text "你好世界" -Out "$env:TEMP\edge-tts-probe.mp3"
#   .\scripts\edge-tts-probe.ps1 -OutputFormat raw-16khz-16bit-mono-pcm -Out "$env:TEMP\pcm.bin"
#
# 参考实现：rany2/edge-tts（Python）；协议细节见 Task/findings.md
# 自检（离线，只验 Sec-MS-GEC v2 令牌算法）：.\scripts\edge-tts-probe.ps1 -SelfTest

param(
    [string]$Text = "你好，欢迎来到直播间",
    [string]$Voice = "zh-CN-XiaoxiaoNeural",
    [string]$Rate = "+0%",
    [string]$Pitch = "+0Hz",
    [string]$Volume = "+0%",
    [string]$OutputFormat = "audio-24khz-48kbitrate-mono-mp3",
    [string]$Out = "$env:TEMP\edge-tts-probe.audio",
    [switch]$DumpHeader,
    [switch]$Quiet,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$TRUSTED_CLIENT_TOKEN = "6A5AA1D4EAFF4E9FB37E23D68491D6F4"
# v2：微软 2026-10 起强制，v1（纯 SHA256 + `1-` 前缀）在部分地区握手上直接 403
$SEC_MS_GEC_VERSION = "2-143.0.3650.75"
$WIN_EPOCH = 11644473600

# ── Sec-MS-GEC v2 令牌 ──────────────────────────────────────────────────────
# 令牌 = `<16 hex CS64>-<64 hex SHA256>`，两套哈希的常数都被刻意改过，
# 不能换成 .NET 的 SHA256。逐行照拄官方 edge-tts 的 src/edge_tts/drm.py，
# 与 Rust 侧 src-tauri/src/tts/edge/drm.rs 是同一份算法（改一处必须同时改另一处）。
$sha256Initial = [uint64[]]@(0x6A09E667L, 0xBB67AE85L, 0x3C6EF372L, 0xA54FF53AL, 0x510E527FL, 0x9B05688CL, 0x1F83D9ABL, 0x5BE0CD19L)
$sha256State = [uint64[]]@(0xFD9FBEA2L, 0x91243CE7L, 0xBE499094L, 0xCD2E1E0AL, 0x6C006F49L, 0x04BD35ABL, 0xFEB0A7E4L, 0x03EFFBD2L)
$sha256Schedule = [uint64[]]@(0xEB24BF9EL, 0x1558691FL, 0xA0C09EE2L, 0x0CFA32BAL, 0xA622EE8CL, 0xD1DD9491L, 0xC0DD25A0L, 0xF9E70AB6L, 0xF0C31573L, 0x7058CE4EL, 0x7188E606L, 0x00E5AB33L, 0xD258B060L, 0x72766564L)
$sha256RoundConstants = [uint64[]]@(
    0x428A2F98L, 0x71374491L, 0xB5C0FBCFL, 0xE9B5DBA5L, 0x3956C25BL, 0x59F111F1L, 0x923F82A4L, 0xAB1C5ED5L,
    0xD807AA98L, 0x12835B01L, 0x243185BEL, 0x550C7DC3L, 0x72BE5D74L, 0x80DEB1FEL, 0x9BDC06A7L, 0xC19BF174L,
    0xE49B69C1L, 0xEFBE4786L, 0x0FC19DC6L, 0x240CA1CCL, 0x2DE92C6FL, 0x4A7484AAL, 0x5CB0A9DCL, 0x76F988DAL,
    0x983E5152L, 0xA831C66DL, 0xB00327C8L, 0xBF597FC7L, 0xC6E00BF3L, 0xD5A79147L, 0x06CA6351L, 0x14292967L,
    0x27B70A85L, 0x2E1B2138L, 0x4D2C6DFCL, 0x53380D13L, 0x650A7354L, 0x766A0ABBL, 0x81C2C92EL, 0x92722C85L,
    0xA2BFE8A1L, 0xA81A664BL, 0xC24B8B70L, 0xC76C51A3L, 0xD192E819L, 0xD6990624L, 0xF40E3585L, 0x106AA070L,
    0x19A4C116L, 0x1E376C08L, 0x2748774CL, 0x34B0BCB5L, 0x391C0CB3L, 0x4ED8AA4AL, 0x5B9CCA4FL, 0x682E6FF3L,
    0x748F82EEL, 0x78A5636FL, 0x84C87814L, 0x8CC70208L, 0x90BEFFFAL, 0xA4506CEBL, 0xBEF9A3F7L, 0xC67178F2L
)
$sha256Adjustment = [uint64]0xC9DC06A7L
$sha256Sigma0Adjustment = [uint64]0x059C0B80L
$sha256Sigma1Adjustment = [uint64]0x000B92C0L

$cs64Coefficients = @(
    @(0xE82E91B1L, 0xB32700D0L), @(0x44DE24C6L, 0x88DFF51DL), @(0x3BEB65BCL, 0x29514038L),
    @(0xC89C5933L, 0xB328CBDFL), @(0x8D1EC211L, 0xD405B4D5L), @(0x314EF5D5L, 0x26E55F3CL),
    @(0xE92491B1L, 0x607801C6L), @(0xF0A5B9D5L, 0x82B2C1FBL), @(0x06064CF3L, 0xFA51A3DAL),
    @(0xDA79FA60L, 0x207CC464L), @(0xBED20560L, 0x3C3C74F3L), @(0x89F9C9F8L, 0xD2D14B61L),
    @(0x35FA38C6L, 0xBB04A2C1L), @(0xCB49E3C6L, 0xC5626167L)
)
$cs64Factor = [uint64]0x82893933L
$cs64Adjustment0 = [uint64]0xF2F90B1AL
$cs64Adjustment1 = [uint64]0x0B1A0000L

function RotL16([uint64]$Value) {
    $v = $Value -band 0xFFFFFFFFL
    return [uint64]((($v -shl 16) -bor ($v -shr 16)) -band 0xFFFFFFFFL)
}

function Get-Sigma0([uint64]$Value) {
    $v = $Value -band 0xFFFFFFFFL
    $r = $v -shr 7
    $r = $r -bxor ($v -shl 25)
    $r = $r -bxor ($v -shr 18)
    $r = $r -bxor ($v -shl 14)
    $r = $r -bxor ($v -shr 3)
    return [uint64]($r -band 0xFFFFFFFFL)
}

function Get-Sigma1([uint64]$Value) {
    $v = $Value -band 0xFFFFFFFFL
    $r = $v -shr 17
    $r = $r -bxor ($v -shl 15)
    $r = $r -bxor ($v -shr 19)
    $r = $r -bxor ($v -shl 13)
    $r = $r -bxor ($v -shr 10)
    return [uint64]($r -band 0xFFFFFFFFL)
}

function Get-ShaRotr([uint64]$X) {
    $r = $X -shr 6
    $r = $r -bxor ($X -shr 11)
    $r = $r -bxor ($X -shr 25)
    return $r
}

function Get-ShaBigSigma0([uint64]$X) {
    $r = $X -shr 2
    $r = $r -bxor ($X -shr 13)
    $r = $r -bxor ($X -shr 22)
    return $r
}

function Get-RoundConstant([int]$Index, [bool]$Block) {
    if ($Block -and $Index -eq 14) { return $sha256Adjustment }
    return $sha256RoundConstants[$Index]
}

# 扩充消息表（sigma 先合并再取模，与标准写法不同）
function Expand-Schedule([uint64[]]$Words, [int]$FirstWord) {
    $w = [System.Collections.Generic.List[uint64]]::new()
    $w.AddRange([uint64[]]$Words)
    for ($index = $FirstWord; $index -lt 64; $index++) {
        $x = $w[$index - 2]
        $y = $w[$index - 15]
        $s1 = ($x -shr 17) -bxor ($x -shl 15) -bxor ($x -shr 19) -bxor ($x -shl 13) -bxor ($x -shr 10)
        $s0 = ($y -shr 7) -bxor ($y -shl 25) -bxor ($y -shr 18) -bxor ($y -shl 14) -bxor ($y -shr 3)
        $w.Add([uint64](($s1 + $w[$index - 7] + $s0 + $w[$index - 16]) -band 0xFFFFFFFFL))
    }
    return $w.ToArray()
}

# 压缩函数。FirstRound 为 14 时改用被替换过常数的那张表（Invoke-Sha256Init 的续算）
function Invoke-Sha256Rounds([uint64[]]$State, [uint64[]]$Words, [int]$FirstRound) {
    $block = ($FirstRound -eq 14)
    $a = $State[0]; $b = $State[1]; $c = $State[2]; $d = $State[3]
    $e = $State[4]; $f = $State[5]; $g = $State[6]; $h = $State[7]
    # 在 64 位里重复一份再右移，等价于 ROTR 且省掉左移那一步
    $aligned = [Math]::Min(64, [int](($FirstRound + 7) / 8) * 8)
    for ($index = $FirstRound; $index -lt $aligned; $index++) {
        $rotE = $e * 0x100000001
        $rotA = $a * 0x100000001
        $t1 = $h + (Get-ShaRotr $rotE) + ($g -bxor ($e -band ($f -bxor $g))) + (Get-RoundConstant $index $block) + $Words[$index]
        $t2 = (Get-ShaBigSigma0 $rotA) + (($a -band $b) -bor ($c -band ($a -bor $b)))
        $na = [uint64](($t1 + $t2) -band 0xFFFFFFFFL)
        $ne = [uint64](($d + $t1) -band 0xFFFFFFFFL)
        $h = $g; $g = $f; $f = $e; $e = $ne; $d = $c; $c = $b; $b = $a; $a = $na
    }
    # 其余轮次按八轮展开，寄存器靠「读谁、写谁」承担角色轮转
    for ($index = $aligned; $index -lt 64; $index += 8) {
        $rotE = $e * 0x100000001
        $rotA = $a * 0x100000001
        $t1 = $h + (Get-ShaRotr $rotE) + ($g -bxor ($e -band ($f -bxor $g))) + (Get-RoundConstant $index $block) + $Words[$index]
        $d = [uint64](($d + $t1) -band 0xFFFFFFFFL)
        $h = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($a -band $b) -bor ($c -band ($a -bor $b)))) -band 0xFFFFFFFFL)

        $rotE = $d * 0x100000001
        $rotA = $h * 0x100000001
        $t1 = $g + (Get-ShaRotr $rotE) + ($f -bxor ($d -band ($e -bxor $f))) + (Get-RoundConstant ($index + 1) $block) + $Words[$index + 1]
        $c = [uint64](($c + $t1) -band 0xFFFFFFFFL)
        $g = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($h -band $a) -bor ($b -band ($h -bor $a)))) -band 0xFFFFFFFFL)

        $rotE = $c * 0x100000001
        $rotA = $g * 0x100000001
        $t1 = $f + (Get-ShaRotr $rotE) + ($e -bxor ($c -band ($d -bxor $e))) + (Get-RoundConstant ($index + 2) $block) + $Words[$index + 2]
        $b = [uint64](($b + $t1) -band 0xFFFFFFFFL)
        $f = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($g -band $h) -bor ($a -band ($g -bor $h)))) -band 0xFFFFFFFFL)

        $rotE = $b * 0x100000001
        $rotA = $f * 0x100000001
        $t1 = $e + (Get-ShaRotr $rotE) + ($d -bxor ($b -band ($c -bxor $d))) + (Get-RoundConstant ($index + 3) $block) + $Words[$index + 3]
        $a = [uint64](($a + $t1) -band 0xFFFFFFFFL)
        $e = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($f -band $g) -bor ($h -band ($f -bor $g)))) -band 0xFFFFFFFFL)

        $rotE = $a * 0x100000001
        $rotA = $e * 0x100000001
        $t1 = $d + (Get-ShaRotr $rotE) + ($c -bxor ($a -band ($b -bxor $c))) + (Get-RoundConstant ($index + 4) $block) + $Words[$index + 4]
        $h = [uint64](($h + $t1) -band 0xFFFFFFFFL)
        $d = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($e -band $f) -bor ($g -band ($e -bor $f)))) -band 0xFFFFFFFFL)

        $rotE = $h * 0x100000001
        $rotA = $d * 0x100000001
        $t1 = $c + (Get-ShaRotr $rotE) + ($b -bxor ($h -band ($a -bxor $b))) + (Get-RoundConstant ($index + 5) $block) + $Words[$index + 5]
        $g = [uint64](($g + $t1) -band 0xFFFFFFFFL)
        $c = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($d -band $e) -bor ($f -band ($d -bor $e)))) -band 0xFFFFFFFFL)

        $rotE = $g * 0x100000001
        $rotA = $c * 0x100000001
        $t1 = $b + (Get-ShaRotr $rotE) + ($a -bxor ($g -band ($h -bxor $a))) + (Get-RoundConstant ($index + 6) $block) + $Words[$index + 6]
        $f = [uint64](($f + $t1) -band 0xFFFFFFFFL)
        $b = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($c -band $d) -bor ($e -band ($c -bor $d)))) -band 0xFFFFFFFFL)

        $rotE = $f * 0x100000001
        $rotA = $b * 0x100000001
        $t1 = $a + (Get-ShaRotr $rotE) + ($h -bxor ($f -band ($g -bxor $h))) + (Get-RoundConstant ($index + 7) $block) + $Words[$index + 7]
        $e = [uint64](($e + $t1) -band 0xFFFFFFFFL)
        $a = [uint64](($t1 + (Get-ShaBigSigma0 $rotA) + (($b -band $c) -bor ($d -band ($b -bor $c)))) -band 0xFFFFFFFFL)
    }
    return [uint64[]]@($a, $b, $c, $d, $e, $f, $g, $h)
}

# 令牌数据的前 7 字节定出初始状态，第 14 轮起接着算（官方把标准常数表切开了）
function Invoke-Sha256Init([byte[]]$Head) {
    $words = [System.Collections.Generic.List[uint64]]::new()
    for ($i = 0; $i -lt 14; $i++) { $words.Add([uint64]0) }
    $w0 = ([uint64]$Head[0] -shl 16) -bor ([uint64]$Head[1] -shl 8) -bor [uint64]$Head[2]
    $words.Add([uint64]$w0)
    $w1 = ([uint64]$Head[3] -shl 24) -bor ([uint64]$Head[4] -shl 16) -bor ([uint64]$Head[5] -shl 8) -bor [uint64]$Head[6]
    $words.Add([uint64]$w1)
    for ($index = 16; $index -lt 30; $index++) {
        $value = $sha256Schedule[$index - 16]
        if ($index - 2 -ge 14) {
            $s1 = Get-Sigma1 $words[$index - 2]
            if ($index -eq 16) { $s1 = $s1 -bxor $sha256Sigma1Adjustment }
            $value += $s1
        }
        if ($index - 7 -ge 14) { $value += $words[$index - 7] }
        if ($index - 15 -ge 14) {
            $s0 = Get-Sigma0 $words[$index - 15]
            if ($index -eq 29) { $s0 = $s0 -bxor $sha256Sigma0Adjustment }
            $value += $s0
        }
        $words.Add([uint64]($value -band 0xFFFFFFFFL))
    }
    $sum = (Get-Sigma1 $words[28]) + $words[23] + (Get-Sigma0 $words[15]) + $words[14] + $sha256Adjustment
    $words.Add([uint64](($sum + (0x100000000 - $sha256RoundConstants[14])) -band 0xFFFFFFFFL))
    $expanded = Expand-Schedule $words.ToArray() 31
    $working = Invoke-Sha256Rounds $sha256State $expanded 14
    $state = [uint64[]]::new(8)
    for ($i = 0; $i -lt 8; $i++) { $state[$i] = [uint64](($sha256Initial[$i] + $working[$i]) -band 0xFFFFFFFFL) }
    return $state
}

# 非标准的 SHA-256：前 7 字节喂进 Invoke-Sha256Init，其余按官方补位规则哈希
function Get-Sha256Digest([byte[]]$Data) {
    if ($Data.Length -lt 7) { throw "令牌数据至少 7 字节" }
    $length = 57 + $Data.Length
    $padded = [System.Collections.Generic.List[byte]]::new()
    for ($i = 7; $i -lt $Data.Length; $i++) { $padded.Add($Data[$i]) }
    $padded.Add(0x80)
    $zeros = ((55 - $length) % 64 + 64) % 64
    for ($i = 0; $i -lt $zeros; $i++) { $padded.Add([byte]0) }
    $bitLength = [uint64]$length * 8
    for ($i = 7; $i -ge 0; $i--) { $padded.Add([byte](($bitLength -shr ($i * 8)) -band 0xFF)) }

    $head = [byte[]]::new(7)
    [Array]::Copy($Data, 0, $head, 0, 7)
    $state = Invoke-Sha256Init $head
    for ($offset = 0; $offset -lt $padded.Count; $offset += 64) {
        $words = [uint64[]]::new(16)
        for ($j = 0; $j -lt 16; $j++) {
            $p = $offset + $j * 4
            $w = ([uint64]$padded[$p] -shl 24) -bor ([uint64]$padded[$p + 1] -shl 16) -bor ([uint64]$padded[$p + 2] -shl 8) -bor [uint64]$padded[$p + 3]
            $words[$j] = [uint64]$w
        }
        $working = Invoke-Sha256Rounds $state (Expand-Schedule $words 16) 0
        for ($i = 0; $i -lt 8; $i++) { $state[$i] = [uint64](($state[$i] + $working[$i]) -band 0xFFFFFFFFL) }
    }
    $out = [byte[]]::new(32)
    for ($i = 0; $i -lt 8; $i++) {
        for ($b = 0; $b -lt 4; $b++) { $out[$i * 4 + $b] = [byte](($state[$i] -shr ((3 - $b) * 8)) -band 0xFF) }
    }
    return $out
}

# 拆成小端 32 位字，尾部补零到 4 字节对齐；字数为奇数时再补一个 0（CS64 要求成对处理）
function Split-WordsLe4Even([byte[]]$Data) {
    $padded = [System.Collections.Generic.List[byte]]::new()
    $padded.AddRange($Data)
    while ($padded.Count % 4 -ne 0) { $padded.Add([byte]0) }
    $words = [System.Collections.Generic.List[uint64]]::new()
    for ($i = 0; $i -lt $padded.Count; $i += 4) {
        $words.Add([uint64]([uint64]$padded[$i] -bor ([uint64]$padded[$i + 1] -shl 8) -bor ([uint64]$padded[$i + 2] -shl 16) -bor ([uint64]$padded[$i + 3] -shl 24)))
    }
    if ($words.Count % 2 -ne 0) { $words.Add([uint64]0) }
    return $words.ToArray()
}

# CS64 摘要：把 14 组系数与令牌字成对折叠回来
function Get-Cs64Digest([byte[]]$Data, [byte[]]$Digest) {
    $keyA = [uint64]$Digest[0] -bor ([uint64]$Digest[1] -shl 8) -bor ([uint64]$Digest[2] -shl 16) -bor ([uint64]$Digest[3] -shl 24)
    $keyA = [uint64]($keyA -bor 1)
    $keyB = [uint64]$Digest[4] -bor ([uint64]$Digest[5] -shl 8) -bor ([uint64]$Digest[6] -shl 16) -bor ([uint64]$Digest[7] -shl 24)
    $keyB = [uint64]($keyB -bor 1)
    # 只需要低 32 位（模乘对低位封闭），降位后乘积必然不溢出 uint64
    $factors = @([uint64](($keyA * $cs64Factor) -band 0xFFFFFFFFL), [uint64](($keyB * $cs64Factor) -band 0xFFFFFFFFL))

    $terms = [System.Collections.Generic.List[uint64]]::new()
    $offsets = [System.Collections.Generic.List[uint64]]::new()
    for ($i = 0; $i -lt $cs64Coefficients.Count; $i++) {
        $terms.Add([uint64](($cs64Coefficients[$i][0] * $factors[$i % 2]) -band 0xFFFFFFFFL))
        $offsets.Add([uint64]$cs64Coefficients[$i][1])
    }

    # 前置一个 0 字节再拆字（与小端拆分的奇偶对齐有关，照拄官方）
    $buf = [System.Collections.Generic.List[byte]]::new()
    $buf.Add([byte]0)
    $buf.AddRange($Data)
    $wordList = Split-WordsLe4Even $buf.ToArray()
    for ($index = 0; $index -lt $wordList.Count; $index++) {
        $word = [uint64]$wordList[$index]
        $key = if ($index % 2 -eq 0) { $keyA } else { $keyB }
        $offsetFactor = if ($index % 2 -eq 0) { 0xEF0569FBL } else { 0xC31713DBL }
        $term = $word * $key
        $offset = (RotL16 $word) * [uint64]$offsetFactor
        if ($index -eq 0) {
            $term += $cs64Adjustment0 * $factors[0]
            $offset += $cs64Adjustment1
        }
        $terms.Add([uint64]($term -band 0xFFFFFFFFL))
        $offsets.Add([uint64]($offset -band 0xFFFFFFFFL))
    }

    $swapAcc = [uint64]0; $swapLast = [uint64]0; $state = [uint64]0; $acc = [uint64]0
    for ($index = 0; $index -lt $terms.Count; $index += 2) {
        $x = $terms[$index]
        $y = $terms[$index + 1]

        $t = RotL16 ([uint64](($offsets[$index] + $x) -band 0xFFFFFFFFL))
        $q = [uint64](($t * 0x76F50F34L) -band 0xFFFFFFFFL)
        $swapAcc = ($swapAcc + [uint64](((RotL16 $q) * 0xC3EFEA97L + $q) -band 0xFFFFFFFFL)) -band 0xFFFFFFFFL

        $t = RotL16 ([uint64](($offsets[$index + 1] + $y) -band 0xFFFFFFFFL))
        $q = [uint64](($t * 0x1881CE3CL) -band 0xFFFFFFFFL)
        $swapLast = [uint64](((RotL16 $q) * 0x35BD1EC9L + $q) -band 0xFFFFFFFFL)
        $swapAcc = ($swapAcc + $swapLast) -band 0xFFFFFFFFL

        $mul = [uint64](($state * $keyA) -band 0xFFFFFFFFL)
        $state = [uint64](($mul + $x) -band 0xFFFFFFFFL)
        $state = [uint64](((RotL16 $state) * 0xCF98B111L) -band 0xFFFFFFFFL)
        $state = [uint64](((RotL16 $state) * 0x87085B9FL) -band 0xFFFFFFFFL)
        $state = [uint64](((RotL16 $state) * 0x12CEB96DL) -band 0xFFFFFFFFL)
        $state = [uint64](((RotL16 $state) * 0x257E1D83L) -band 0xFFFFFFFFL)
        $acc = ($acc + $state) -band 0xFFFFFFFFL

        $mul = [uint64](($state * $keyB) -band 0xFFFFFFFFL)
        $t = [uint64](($mul + $y) -band 0xFFFFFFFFL)
        $t = [uint64](((RotL16 $t) * 0xA27416F5L) -band 0xFFFFFFFFL)
        $t = [uint64](((RotL16 $t) * 0xD38396FFL) -band 0xFFFFFFFFL)
        $t = [uint64](((RotL16 $t) * 0x7C932B89L) -band 0xFFFFFFFFL)
        $state = [uint64](((RotL16 $t) * 0xBFA49F69L) -band 0xFFFFFFFFL)
        $acc = ($acc + $state) -band 0xFFFFFFFFL
    }
    return @([uint64]($swapLast -bxor $state), [uint64]($swapAcc -bxor $acc))
}

function New-SecMsGec([double]$UnixTimestamp = 0) {
    if ($UnixTimestamp -le 0) { $UnixTimestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() }
    # 切到 Windows FILETIME 纪元、向下取整到 5 分钟、转 100ns 单位（保持浮点路径，见文件头说明）
    $ticks = $UnixTimestamp + $WIN_EPOCH
    $ticks = $ticks - ($ticks % 300)
    $ticks = $ticks * 1e7
    $raw = "{0:F0}{1}" -f $ticks, $TRUSTED_CLIENT_TOKEN
    $bytes = [Text.Encoding]::ASCII.GetBytes($raw)

    $digest = Get-Sha256Digest $bytes
    $cs64 = Get-Cs64Digest $bytes $digest
    # CS64 按小端拼字节后再取 hex（不能直接格式化整数）
    $cs64Bytes = [byte[]]::new(8)
    for ($i = 0; $i -lt 4; $i++) {
        $cs64Bytes[$i] = [byte](($cs64[0] -shr ($i * 8)) -band 0xFF)
        $cs64Bytes[4 + $i] = [byte](($cs64[1] -shr ($i * 8)) -band 0xFF)
    }
    $cs64Hex = (($cs64Bytes | ForEach-Object { $_.ToString("X2") }) -join "")
    $shaHex = (($digest | ForEach-Object { $_.ToString("X2") }) -join "")
    return "$cs64Hex-$shaHex"
}

# 离线自检：官方从 Edge 抓包得到的向量（rany2/edge-tts#492），逐字节锁定两套哈希
if ($SelfTest) {
    $actual = New-SecMsGec -UnixTimestamp 1791888000
    $expect = "492D7694A183A45E-56E821F95D4579C5B7C7E33DC7303FF379EC36D510213555BBBB9EB17EF15876"
    if ($actual -eq $expect) {
        Write-Host "✓ Sec-MS-GEC v2 令牌自检通过：$actual" -ForegroundColor Green
        exit 0
    }
    Write-Host "✗ Sec-MS-GEC v2 令牌自检失败" -ForegroundColor Red
    Write-Host "  实际：$actual"
    Write-Host "  期望：$expect"
    exit 1
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
