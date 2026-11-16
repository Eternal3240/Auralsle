# Auralsle AI-status mod - Universal AI Agent & WorkBuddy status hook wrapper.
# Supports WorkBuddy, Claude Code, Codex CLI, Cursor, and Windsurf hooks.
# Writes UTF-8 (no BOM) status JSON directly to all Auralsle / ZenDesktop status
# directories so it works out-of-the-box even without zen-status.exe.

param(
    [string]$Agent = 'workbuddy',
    [string]$Name = 'WorkBuddy',
    [string]$State = '',
    [string]$Message = '',
    [switch]$WaitApproval
)

$ErrorActionPreference = 'SilentlyContinue'

$raw = ''
try {
    if ([Console]::IsInputRedirected) {
        $stdin = [Console]::OpenStandardInput()
        $reader = New-Object System.IO.StreamReader($stdin, [System.Text.Encoding]::UTF8)
        $raw = $reader.ReadToEnd()
    }
} catch { }

$event = ''
$prompt = ''
$cwd = ''
$toolName = ''
$toolDetail = ''

if ($raw -and $raw.Trim().Length -gt 0) {
    try {
        $j = $raw | ConvertFrom-Json
        if ($j.hook_event_name) { $event = [string]$j.hook_event_name }
        elseif ($j.event) { $event = [string]$j.event }
        elseif ($j.type) { $event = [string]$j.type }

        if ($j.prompt) { $prompt = [string]$j.prompt }
        elseif ($j.user_prompt) { $prompt = [string]$j.user_prompt }
        elseif ($j.message) { $prompt = [string]$j.message }
        elseif ($j.'last-assistant-message') { $prompt = [string]$j.'last-assistant-message' }

        if ($j.cwd) { $cwd = [string]$j.cwd }
        if ($j.tool_name) { $toolName = [string]$j.tool_name }
        if ($j.tool_input) {
            if ($j.tool_input.command) { $toolDetail = [string]$j.tool_input.command }
            elseif ($j.tool_input.file_path) { $toolDetail = [string]$j.tool_input.file_path }
            elseif ($j.tool_input.path) { $toolDetail = [string]$j.tool_input.path }
            elseif ($j.tool_input.pattern) { $toolDetail = [string]$j.tool_input.pattern }
            elseif ($j.tool_input.description) { $toolDetail = [string]$j.tool_input.description }
        }
    } catch { }
}

$stepText = ''
if (-not $State) {
    switch -Regex ($event) {
        '^(UserPromptSubmit|SessionStart|SubagentStart)$' {
            $State = 'working'
            $stepText = 'Thinking & planning...'
        }
        '^(PreToolUse)$' {
            if ($WaitApproval) {
                $State = 'waiting'
                $stepText = if ($toolName) { "Wait approval: $toolName" } else { 'Waiting for approval' }
            } else {
                $State = 'tool'
                $stepText = if ($toolName) { "Tool: $toolName" } else { 'Running tool...' }
            }
        }
        '^(PermissionRequest)$' {
            $State = 'waiting'
            $stepText = if ($toolName) { "Approve: $toolName" } else { 'Waiting for permission' }
        }
        '^(PostToolUse|PostToolUseFailure)$' {
            $State = 'working'
            $stepText = if ($toolName) { "Finished: $toolName" } else { 'Processing results...' }
        }
        '^(Notification)$' {
            $State = 'waiting'
            $stepText = 'Waiting for user input'
        }
        '^(Stop|SubagentStop|SessionEnd|agent-turn-complete|task_complete)$' {
            $State = 'done'
            $stepText = 'Completed'
        }
        default {
            $State = 'working'
        }
    }
}

$msgLine = $Message
if (-not $msgLine) {
    if ($toolDetail) { $msgLine = $toolDetail }
    elseif ($prompt) { $msgLine = $prompt }
    elseif ($toolName) { $msgLine = "Using $toolName" }
}
if ($msgLine) {
    $msgLine = ($msgLine -replace '\s+', ' ').Trim()
    if ($msgLine.Length -gt 64) { $msgLine = $msgLine.Substring(0, 64) + '...' }
}

$project = ''
if ($cwd) {
    try { $project = Split-Path -Leaf $cwd } catch { }
}

$nowUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$payloadObj = [ordered]@{
    agent   = $Agent
    name    = $Name
    state   = $State
    message = [string]$msgLine
    step    = [string]$stepText
    project = [string]$project
    updated = $nowUnix
}
$jsonText = $payloadObj | ConvertTo-Json -Compress
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

$statusDirs = @()
if ($PSScriptRoot) {
    $parentDir = Split-Path $PSScriptRoot -Parent
    if ($parentDir) { $statusDirs += (Join-Path $parentDir 'ai-status') }
}
if ($env:APPDATA) {
    $statusDirs += (Join-Path $env:APPDATA 'Auralsle\ai-status')
}
if ($env:LOCALAPPDATA) {
    $statusDirs += (Join-Path $env:LOCALAPPDATA 'ZenDesktop\ai-status')
}

foreach ($sDir in ($statusDirs | Select-Object -Unique)) {
    try {
        New-Item -ItemType Directory -Force -Path $sDir | Out-Null
        if ($WaitApproval -or $State -eq 'waiting') {
            $apFile = Join-Path $sDir "$Agent.approval.json"
            if (Test-Path $apFile) { Remove-Item $apFile -Force -ErrorAction SilentlyContinue }
        }
        $targetJson = Join-Path $sDir "$Agent.json"
        [System.IO.File]::WriteAllText($targetJson, $jsonText, $utf8NoBom)
    } catch { }
}

# Also forward to zen-status.exe if installed on the system
$zen = Join-Path $PSScriptRoot 'zen-status.exe'
if (-not (Test-Path $zen) -and $env:LOCALAPPDATA) {
    $old = Join-Path $env:LOCALAPPDATA 'ZenDesktop\ai-status-tools\zen-status.exe'
    if (Test-Path $old) { $zen = $old }
}
if (Test-Path $zen) {
    try {
        if ($event -eq 'UserPromptSubmit' -and $msgLine) {
            if ($project) {
                & $zen $Agent working $msgLine --name $Name --project $project 2>$null
            } else {
                & $zen $Agent working $msgLine --name $Name 2>$null
            }
        } elseif ($raw) {
            $raw | & $zen hook $Agent --name $Name 2>$null
        }
    } catch { }
}

if ($WaitApproval) {
    for ($i = 0; $i -lt 240; $i++) {
        Start-Sleep -Milliseconds 250
        foreach ($sDir in ($statusDirs | Select-Object -Unique)) {
            $apFile = Join-Path $sDir "$Agent.approval.json"
            if (Test-Path $apFile) {
                try {
                    $resp = Get-Content $apFile -Raw -Encoding UTF8 | ConvertFrom-Json
                    Remove-Item $apFile -Force -ErrorAction SilentlyContinue
                    if ($resp.decision -eq 'allow') { exit 0 }
                    elseif ($resp.decision -eq 'deny') {
                        [Console]::Error.WriteLine('Denied by Auralsle Dynamic Island')
                        exit 2
                    } else { exit 1 }
                } catch { exit 0 }
            }
        }
    }
}

exit 0
