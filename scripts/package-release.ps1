$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$frontendRoot = Join-Path $repoRoot "frontend"
$tauriCli = Join-Path $frontendRoot "node_modules\.bin\tauri.cmd"
$releaseExe = Join-Path $repoRoot "target\release\codex-launcher.exe"
$distDir = Join-Path $repoRoot "dist"
$distExe = Join-Path $distDir "codex-launcher.exe"
$distSha = Join-Path $distDir "codex-launcher.exe.sha256"

Set-Location $repoRoot

if (!(Test-Path $tauriCli)) {
    Write-Host "Tauri CLI not found in frontend/node_modules; installing frontend dependencies..."
    & npm --prefix frontend install
    if ($LASTEXITCODE -ne 0) {
        throw "npm install failed with exit code $LASTEXITCODE"
    }
}

if (!(Test-Path $tauriCli)) {
    throw "Tauri CLI is still missing: $tauriCli"
}

Write-Host "Building release exe with Tauri..."
& $tauriCli build --no-bundle
if ($LASTEXITCODE -ne 0) {
    throw "Tauri build failed with exit code $LASTEXITCODE"
}

if (!(Test-Path $releaseExe)) {
    throw "Release exe was not produced: $releaseExe"
}

$exeItem = Get-Item $releaseExe
if ($exeItem.Length -lt 4000000) {
    throw "Release exe looks too small ($($exeItem.Length) bytes): $releaseExe"
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
Copy-Item -Force $releaseExe $distExe

$hash = (Get-FileHash $distExe -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  codex-launcher.exe" | Out-File -FilePath $distSha -Encoding ASCII -NoNewline

Write-Host ""
Write-Host "Release package ready:"
Write-Host "  $distExe"
Write-Host "  $distSha"
Write-Host "  Size: $($exeItem.Length) bytes"
Write-Host "  SHA256: $hash"
