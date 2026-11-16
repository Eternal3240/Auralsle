$ErrorActionPreference='Stop'
$installDir=[System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\AuraIsle'))
if([System.IO.Path]::GetFullPath($PSScriptRoot) -ne $installDir){throw '卸载脚本只能在灵屿 AuraIsle 安装目录中运行。'}
$exePath=Join-Path $installDir 'AuraIsle.exe'
$legacyExePath=Join-Path $installDir 'auralsle.exe'
Get-Process AuraIsle, auralsle -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $exePath -or $_.Path -eq $legacyExePath} | ForEach-Object {
    $null=$_.CloseMainWindow();if(-not $_.WaitForExit(3000)){Stop-Process -Id $_.Id}
}
$shell=New-Object -ComObject WScript.Shell
$links=@(
    (Join-Path $shell.SpecialFolders.Item('Desktop') '灵屿 AuraIsle.lnk'),
    (Join-Path $shell.SpecialFolders.Item('Desktop') '岛仔.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\灵屿 AuraIsle.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\灵屿设置.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\收集灵屿诊断.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\岛仔.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\岛仔设置.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\收集岛仔诊断.lnk')
)
foreach($path in $links){
    if(Test-Path -LiteralPath $path){
        $tp = $shell.CreateShortcut($path).TargetPath
        if($tp -eq $exePath -or $tp -eq $legacyExePath){Remove-Item -LiteralPath $path}
    }
}
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Remove-ItemProperty -Path $runKey -Name AuraIsle -ErrorAction SilentlyContinue
Remove-ItemProperty -Path $runKey -Name Auralsle -ErrorAction SilentlyContinue
Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AuraIsle' -ErrorAction SilentlyContinue
Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Auralsle' -ErrorAction SilentlyContinue
foreach($name in @('AuraIsle.exe','auralsle.exe','AuraIsle-previous.exe','auralsle-previous.exe','auralsle-v0.2-backup.exe','THIRD_PARTY_NOTICES.txt','LICENSE','uninstall.ps1')){
    $path=Join-Path $installDir $name;if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}
}
if(-not(Get-ChildItem -LiteralPath $installDir -Force)){Remove-Item -LiteralPath $installDir}
Write-Output '灵屿 AuraIsle 已卸载。待办、饮水记录和设置仍保存在本机用户数据目录中。'
