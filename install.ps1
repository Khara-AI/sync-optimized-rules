# Install or update to the latest sor release (Windows).
#   irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex
# Set $env:SOR_PLUGIN = 1 before the same command to install the Claude Code
# plugin too, $env:SOR_UNINSTALL = 1 to remove sor (with SOR_PLUGIN, the plugin as well).
# $env:SOR_INSTALL_DIR overrides the target folder, $env:SOR_VERSION pins a release,
# $env:SOR_MARKETPLACE installs the plugin from another source (a local checkout).
$ErrorActionPreference = "Stop"

$repo = "Khara-AI/sync-optimized-rules"
$dest = if ($env:SOR_INSTALL_DIR) { $env:SOR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\sor" }
$marketplace = if ($env:SOR_MARKETPLACE) { $env:SOR_MARKETPLACE } else { $repo }

function Assert-Claude($command) {
    if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
        throw "sor: claude was not found. Install Claude Code, then run: claude plugin $command"
    }
}

if ($env:SOR_UNINSTALL) {
    $exe = Join-Path $dest "sor.exe"
    if (Test-Path $exe) { Remove-Item $exe; Write-Host "sor removed from $dest." }
    else { Write-Host "sor is not installed in $dest." }
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $kept = ($userPath -split ';') | Where-Object { $_ -and $_ -ne $dest }
    [Environment]::SetEnvironmentVariable("Path", ($kept -join ';'), "User")
    if ($env:SOR_PLUGIN) {
        Assert-Claude "uninstall sor@sync-optimized-rules"
        claude plugin uninstall sor@sync-optimized-rules
        claude plugin marketplace remove sync-optimized-rules
    }
    Write-Host "Your configuration is left in $(Join-Path $env:APPDATA 'sync-optimized-rules')."
    return
}
if ($env:SOR_PLUGIN) { Assert-Claude "install sor --marketplace $repo" }
$base = if ($env:SOR_VERSION) { "https://github.com/$repo/releases/download/v$($env:SOR_VERSION)" }
        else { "https://github.com/$repo/releases/latest/download" }
$asset = "sor-x86_64-pc-windows-msvc.zip"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Invoke-WebRequest "$base/$asset" -OutFile "$tmp\$asset"
    Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS"
    $line = Select-String -Path "$tmp\SHA256SUMS" -Pattern " $([regex]::Escape($asset))$"
    $expected = if ($line) { ($line.Line -split ' ')[0] } else { "" }
    $actual = (Get-FileHash "$tmp\$asset" -Algorithm SHA256).Hash.ToLower()
    if (-not $expected -or $expected -ne $actual) { throw "sor: checksum mismatch for $asset" }

    Expand-Archive "$tmp\$asset" -DestinationPath $tmp
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item "$tmp\sor.exe" (Join-Path $dest "sor.exe") -Force
    Write-Host "sor installed in $dest\sor.exe"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($userPath -split ';') -notcontains $dest) {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$dest", "User")
        Write-Host "$dest added to your PATH (open a new terminal)."
    }
} finally {
    Remove-Item -Recurse -Force $tmp
}

if ($env:SOR_PLUGIN) {
    claude plugin install sor --marketplace $marketplace
    if ($LASTEXITCODE -ne 0) { throw "sor: the plugin install failed" }
    Write-Host "In Claude Code, run /reload-plugins (or restart it), then /sor:sor."
}
