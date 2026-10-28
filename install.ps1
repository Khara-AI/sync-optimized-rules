# Install or update to the latest sor release (Windows).
#   irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex
# Uninstall: set $env:SOR_UNINSTALL = 1 before the same command.
# $env:SOR_INSTALL_DIR overrides the target folder, $env:SOR_VERSION pins a release.
$ErrorActionPreference = "Stop"

$repo = "Khara-AI/sync-optimized-rules"
$dest = if ($env:SOR_INSTALL_DIR) { $env:SOR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\sor" }

if ($env:SOR_UNINSTALL) {
    $exe = Join-Path $dest "sor.exe"
    if (Test-Path $exe) { Remove-Item $exe; Write-Host "sor removed from $dest." }
    else { Write-Host "sor is not installed in $dest." }
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $kept = ($userPath -split ';') | Where-Object { $_ -and $_ -ne $dest }
    [Environment]::SetEnvironmentVariable("Path", ($kept -join ';'), "User")
    Write-Host "Your configuration is left in $(Join-Path $env:APPDATA 'sync-optimized-rules')."
    return
}
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
