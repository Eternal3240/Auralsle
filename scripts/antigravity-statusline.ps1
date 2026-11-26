# 只保存 Antigravity CLI (`agy`) 通过标准输入提供的真实套餐额度与模型配额，不读取登录令牌或发送网络请求。
$ErrorActionPreference = 'Stop'
try {
    $raw = [Console]::In.ReadToEnd()
    if ([string]::IsNullOrWhiteSpace($raw)) { exit 0 }
    $data = $raw | ConvertFrom-Json

    $hasLimits = $false
    $limits = [ordered]@{}
    if ($null -ne $data.rate_limits) {
        foreach ($key in @('five_hour', 'seven_day')) {
            $window = $data.rate_limits.$key
            if ($null -eq $window) { continue }
            $used = $null
            if ($window.used_percentage -is [ValueType] -and $window.used_percentage -isnot [bool]) {
                $u = [double]$window.used_percentage
                if ($u -ge 0 -and $u -le 100) { $used = $u }
            } elseif ($window.remaining_fraction -is [ValueType] -and $window.remaining_fraction -isnot [bool]) {
                $rf = [double]$window.remaining_fraction
                if ($rf -ge 0 -and $rf -le 1) { $used = (1.0 - $rf) * 100.0 }
            }
            if ($null -ne $used) {
                $limits[$key] = @{
                    used_percentage = [math]::Round($used, 4)
                    resets_at = $window.resets_at
                }
                $hasLimits = $true
            }
        }
    }

    $hasBuckets = ($null -ne $data.buckets -and @($data.buckets).Count -gt 0)
    $hasGroups = ($null -ne $data.groups -and @($data.groups).Count -gt 0) -or ($null -ne $data.response -and $null -ne $data.response.groups -and @($data.response.groups).Count -gt 0)
    $hasQuota = ($null -ne $data.quota) -or ($null -ne $data.models)
    $hasCredits = ($null -ne $data.credits) -or ($null -ne $data.ai_credits) -or ($null -ne $data.availablePromptCredits)

    if ($hasLimits -or $hasBuckets -or $hasGroups -or $hasQuota -or $hasCredits) {
        $record = [ordered]@{
            version = 1
            source = 'agy-statusline'
            at = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        }
        $tierVal = if ($data.plan_tier) { $data.plan_tier } elseif ($data.user_tier) { $data.user_tier } elseif ($data.tier) { $data.tier } else { $null }
        if ($tierVal) { $record['plan_tier'] = [string]$tierVal }
        if ($data.email) { $record['email'] = [string]$data.email }
        if ($hasLimits) { $record['rate_limits'] = $limits }
        if ($hasBuckets) { $record['buckets'] = $data.buckets }
        if ($null -ne $data.groups) { $record['groups'] = $data.groups }
        if ($null -ne $data.response) { $record['response'] = $data.response }
        if ($null -ne $data.quota) { $record['quota'] = $data.quota }
        if ($null -ne $data.models) { $record['models'] = $data.models }
        if ($null -ne $data.credits) { $record['credits'] = $data.credits }
        if ($null -ne $data.ai_credits) { $record['ai_credits'] = $data.ai_credits }
        if ($null -ne $data.availablePromptCredits) { $record['availablePromptCredits'] = $data.availablePromptCredits }
        if ($null -ne $data.monthlyPromptCredits) { $record['monthlyPromptCredits'] = $data.monthlyPromptCredits }

        $path = Join-Path $PSScriptRoot 'auralsle-usage.json'
        $temporary = $path + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
        try {
            [IO.File]::WriteAllText($temporary, ($record | ConvertTo-Json -Depth 10 -Compress), [Text.UTF8Encoding]::new($false))
            Move-Item -LiteralPath $temporary -Destination $path -Force
        } finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -ErrorAction SilentlyContinue }
        }
    }

    $parts = @($data.model.display_name)
    if ($limits.Contains('five_hour')) { $parts += '5h: ' + [math]::Round((100.0 - [double]$limits['five_hour'].used_percentage), 0) + '%' }
    if ($limits.Contains('seven_day')) { $parts += '7d: ' + [math]::Round((100.0 - [double]$limits['seven_day'].used_percentage), 0) + '%' }
    [Console]::WriteLine(($parts | Where-Object { $_ }) -join ' | ')
} catch {
    # 缺少配额字段或异常时保持安静，不触发登录或网络请求。
    exit 0
}
