# Copy built agent binaries into dist/agents for portal download buttons.
param(
    [string]$OutDir = "$PSScriptRoot\..\dist\agents"
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\.."
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$win = Join-Path $targetDir "release\dice-agent.exe"
if (-not (Test-Path $win)) {
    Write-Host "Building release agent..."
    Push-Location $root
    cargo build --release -p dice-agent
    Pop-Location
}
# Refresh path in case CARGO_TARGET_DIR was set by the environment during build
if (-not (Test-Path $win) -and $env:CARGO_TARGET_DIR) {
    $win = Join-Path $env:CARGO_TARGET_DIR "release\dice-agent.exe"
}

if (Test-Path $win) {
    Copy-Item $win (Join-Path $OutDir "dice-agent-windows-x64.exe") -Force
    Copy-Item $win (Join-Path $OutDir "dice-agent.exe") -Force
    Write-Host "Published Windows agent -> $OutDir ($win)"
} else {
    Write-Warning "Windows binary missing. Expected under $targetDir\release\dice-agent.exe"
}

$linux = Join-Path $targetDir "release\dice-agent"
if (Test-Path $linux) {
    Copy-Item $linux (Join-Path $OutDir "dice-agent-linux-x64") -Force
    Write-Host "Published Linux agent"
}

Copy-Item (Join-Path $root "deploy\install-agent.ps1") (Join-Path $OutDir "install-agent.ps1") -Force
Copy-Item (Join-Path $root "deploy\install-agent.sh") (Join-Path $OutDir "install-agent.sh") -Force

@"
server_url = "https://backup.example.com"
agent_token = "paste-token-from-admin"
enabled = true
poll_interval_secs = 30
extra_paths = []
"@ | Set-Content -Encoding UTF8 (Join-Path $OutDir "dice-agent.toml.example")

Get-ChildItem $OutDir | Format-Table Name, Length
