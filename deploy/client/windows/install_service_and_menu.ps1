# Windows 客户端后台常驻服务与右键菜单【同步到... >】注册安装脚本
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\multinteractor"
)

$ErrorActionPreference = "Stop"
Write-Host "[1/3] Preparing multinteractor directory at $InstallDir ..."
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path "$env:APPDATA\multinteractor" | Out-Null

Write-Host "[2/3] Registering Windows Explorer Context Menu [同步到... >] ..."
$Scopes = @("HKCU:\Software\Classes\*\shell\MultinteractorSync", "HKCU:\Software\Classes\Directory\shell\MultinteractorSync")
foreach ($KeyPath in $Scopes) {
    New-Item -Path $KeyPath -Force | Out-Null
    Set-ItemProperty -Path $KeyPath -Name "MUIVerb" -Value "同步到... >"
    New-Item -Path "$KeyPath\command" -Force | Out-Null
    Set-ItemProperty -Path "$KeyPath\command" -Name "(Default)" -Value "`"$InstallDir\mi-ui.exe`" --pick-remote-dir --source `"%1`""
}

Write-Host "[3/3] Registering Startup Task for mi-daemon ..."
$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty -Path $RunKey -Name "MultinteractorDaemon" -Value "`"$InstallDir\mi-daemon.exe`""

Write-Host "Installation completed successfully."
