param([Parameter(Mandatory=$true)][string]$WorkPath, [Parameter(Mandatory=$true)][string]$TargetExe, [int]$ParentProcessId)
$ErrorActionPreference = 'Stop'
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
$workRoot = [IO.Path]::GetFullPath($WorkPath).TrimEnd('\')
$target = [IO.Path]::GetFullPath($TargetExe)
$targetDir = Split-Path $target -Parent
$stagedInWork = Join-Path $workRoot 'staged.exe'
$backup = Join-Path $workRoot 'previous.exe'
$errFile = Join-Path $workRoot 'error.txt'
$replaced = $false
$exited = $false

function Write-UpdateError([string]$msg) {
    try {
        if (Test-Path -LiteralPath $workRoot -PathType Container) {
            [IO.File]::WriteAllText($errFile, $msg, [Text.UTF8Encoding]::new($false))
        }
    } catch { }
}

function Test-DirWritable([string]$dir) {
    $probe = Join-Path $dir ('.auraisle-write-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllBytes($probe, [byte[]]@(0))
        Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
        return $true
    } catch {
        return $false
    }
}

try {
    if (-not (Test-Path -LiteralPath $workRoot -PathType Container)) { throw '更新缓存目录不存在。' }
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "找不到当前程序：$target" }

    $version = (Get-Content -LiteralPath (Join-Path $workRoot 'version.txt') -Raw).Trim()
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "更新版本号无效：$version" }

    $archive = Join-Path $workRoot 'update.zip'
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw '更新压缩包不存在。' }

    $expectedHash = (Get-Content -LiteralPath (Join-Path $workRoot 'hash.txt') -Raw).Trim()
    $actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($expectedHash -notmatch '^[a-fA-F0-9]{64}$' -or -not [string]::Equals($actualHash, $expectedHash, [StringComparison]::OrdinalIgnoreCase)) {
        throw '更新包 SHA256 校验失败。'
    }

    # 1) 在缓存目录中解压主程序（支持任意目录层级或根目录中的 AuraIsle.exe / auralsle.exe，不受目标目录权限限制）
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        # 拒绝路径穿越 / 绝对路径条目，避免解压写到目标目录之外。
        $badEntries = @($zip.Entries | Where-Object {
            $n = ($_.FullName -replace '\\', '/')
            $n.StartsWith('/') -or $n -match '^[A-Za-z]:' -or $n -match '(^|/)\.\.(/|$)'
        })
        if ($badEntries.Count -gt 0) {
            throw "更新包包含非法路径：$($badEntries[0].FullName)"
        }
        $entries = @($zip.Entries | Where-Object {
            $leaf = [IO.Path]::GetFileName($_.FullName.Replace('\', '/'))
            $_.Length -gt 0 -and $_.Length -le 200MB -and (
                [string]::Equals($leaf, 'AuraIsle.exe', [StringComparison]::OrdinalIgnoreCase) -or
                [string]::Equals($leaf, 'auralsle.exe', [StringComparison]::OrdinalIgnoreCase)
            )
        })
        if ($entries.Count -ne 1) {
            throw "更新包中未找到唯一的 AuraIsle.exe（匹配数量：$($entries.Count)）。"
        }
        if (Test-Path -LiteralPath $stagedInWork) { Remove-Item -LiteralPath $stagedInWork -Force }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $stagedInWork, $true)
    } finally {
        $zip.Dispose()
    }

    # 2) 校验版本号（兼容 FileVersion 与 ProductVersion，容忍 .0 后缀）
    $verOk = $false
    $actualVer = ''
    for ($tryVer = 0; $tryVer -lt 6 -and -not $verOk; $tryVer++) {
        $vi = [Diagnostics.FileVersionInfo]::GetVersionInfo($stagedInWork)
        $fv = if ($vi.FileVersion) { $vi.FileVersion.Trim() } else { '' }
        $pv = if ($vi.ProductVersion) { $vi.ProductVersion.Trim() } else { '' }
        $actualVer = if ($fv) { $fv } else { $pv }
        if ($fv -eq $version -or $fv -eq "$version.0" -or $pv -eq $version -or $pv -eq "$version.0") {
            $verOk = $true
        } else {
            Start-Sleep -Milliseconds 150
        }
    }
    if (-not $verOk -and $actualVer) {
        throw "安装包版本不匹配（期望 $version，实际 $actualVer）。"
    }

    # 3) 预检查目标目录是否可写（若安装在 Program Files 等受保护目录，则后续提权替换）
    $needsElevation = -not (Test-DirWritable $targetDir)

    # 通知主程序准备完成，可以安全退出
    [IO.File]::WriteAllText((Join-Path $workRoot 'ready'), 'ok', [Text.UTF8Encoding]::new($false))

    if ($ParentProcessId -gt 0) {
        $parent = Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue
        if ($parent -and -not $parent.WaitForExit(60000)) { throw '灵屿 AuraIsle 尚未退出，请稍后重试。' }
    }
    $exited = $true
    # 留出短暂缓冲，等待 Windows / 杀毒软件释放旧 exe 文件句柄
    Start-Sleep -Milliseconds 250

    if ($needsElevation) {
        $elevScript = Join-Path $workRoot 'elevated-replace.ps1'
        $elevBody = @"
`$ErrorActionPreference = 'Stop'
`$src = '$($stagedInWork.Replace("'", "''"))'
`$dst = '$($target.Replace("'", "''"))'
for (`$i = 0; `$i -lt 15; `$i++) {
    try {
        [IO.File]::Copy(`$src, `$dst, `$true)
        exit 0
    } catch {
        Start-Sleep -Milliseconds 300
    }
}
throw '无法覆盖受保护目录中的主程序文件。'
"@
        [IO.File]::WriteAllText($elevScript, $elevBody, [Text.UTF8Encoding]::new($true))
        $proc = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -Verb RunAs -PassThru -Wait -WindowStyle Hidden -ArgumentList @(
            '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $elevScript
        )
        if (-not $proc -or $proc.ExitCode -ne 0) {
            throw '管理员提权覆盖失败或已取消。'
        }
        $replaced = $true
    } else {
        [IO.File]::Copy($target, $backup, $true)
        $copied = $false
        $lastErr = ''
        for ($attempt = 0; $attempt -lt 15 -and -not $copied; $attempt++) {
            try {
                [IO.File]::Copy($stagedInWork, $target, $true)
                $copied = $true
            } catch {
                $lastErr = $_.Exception.Message
                Start-Sleep -Milliseconds 300
            }
        }
        if (-not $copied) { throw "无法覆盖主程序：$lastErr" }
        $replaced = $true
    }

    Start-Process -FilePath $target -WorkingDirectory $targetDir -WindowStyle Hidden

    foreach ($key in @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AuraIsle',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Auralsle'
    )) {
        $registered = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
        if ($registered -and $registered.InstallLocation -and [string]::Equals($registered.InstallLocation.TrimEnd('\'), $targetDir, [StringComparison]::OrdinalIgnoreCase)) {
            Set-ItemProperty -LiteralPath $key -Name DisplayVersion -Value $version -ErrorAction SilentlyContinue
            $own = @('AuraIsle.exe','auralsle.exe','Uninstall.exe','LICENSE','THIRD_PARTY_NOTICES.txt') | ForEach-Object { Join-Path $targetDir $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }
            $sizeKb = [int][Math]::Ceiling((($own | ForEach-Object { (Get-Item -LiteralPath $_).Length }) | Measure-Object -Sum).Sum / 1KB)
            if ($sizeKb -gt 0) { Set-ItemProperty -LiteralPath $key -Name EstimatedSize -Value $sizeKb -Type DWord -ErrorAction SilentlyContinue }
        }
    }

    try { Remove-Item -LiteralPath $workRoot -Recurse -Force -ErrorAction SilentlyContinue } catch { }
} catch {
    $message = $_.Exception.Message
    if ($replaced -and (Test-Path -LiteralPath $backup)) {
        try { [IO.File]::Copy($backup, $target, $true) } catch { }
    }
    if (Test-Path -LiteralPath $stagedInWork) {
        Remove-Item -LiteralPath $stagedInWork -Force -ErrorAction SilentlyContinue
    }
    if (-not $exited) {
        Write-UpdateError $message
        exit 1
    }
    if ($exited) {
        Start-Process -FilePath $target -WorkingDirectory $targetDir -WindowStyle Hidden
    }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.MessageBox]::Show("更新未完成：$message`r`n原版本已保留，可稍后重试或通过浏览器下载。", '灵屿 AuraIsle 更新') | Out-Null
    } catch { }
    exit 1
}
