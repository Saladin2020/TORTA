# Install TORTA Dice agent on Windows (copy binary + Scheduled Task)
param(
    [Parameter(Mandatory = $true)][string]$ServerUrl,
    [Parameter(Mandatory = $true)][string]$AgentToken,
    [string]$InstallDir = "$env:LOCALAPPDATA\TortaDice",
    # Default is tray UI. Pass -Headless for background-only.
    [switch]$Headless
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path "$InstallDir\config" | Out-Null

$binSrc = Join-Path $PSScriptRoot "..\target\release\dice-agent.exe"
if (-not (Test-Path $binSrc)) {
    $alt = Join-Path $PSScriptRoot "..\dist\agents\dice-agent-windows-x64.exe"
    if (Test-Path $alt) { $binSrc = $alt }
    else { Write-Error "Build first: cargo build --release -p dice-agent (or run deploy/publish-agent.ps1)" }
}

Copy-Item $binSrc "$InstallDir\dice-agent.exe" -Force

@"
server_url = "$ServerUrl"
agent_token = "$AgentToken"
enabled = true
poll_interval_secs = 30
extra_paths = []
"@ | Set-Content -Encoding UTF8 "$InstallDir\config\dice-agent.toml"

$mode = if ($Headless) { "run" } else { "tray" }
$action = New-ScheduledTaskAction -Execute "$InstallDir\dice-agent.exe" -Argument "-c `"$InstallDir\config\dice-agent.toml`" $mode"
$trigger = New-ScheduledTaskTrigger -AtLogOn
Register-ScheduledTask -TaskName "TortaDiceAgent" -Action $action -Trigger $trigger -Force | Out-Null
Write-Host "Installed. Task: TortaDiceAgent (mode=$mode)"
Write-Host "Config: $InstallDir\config\dice-agent.toml"
Write-Host "Tray menu: sync / pause / connection settings / open portal / quit"
