param([switch]$Offline, [switch]$UseMirror)
$ErrorActionPreference = 'Stop'
$lessonRoot = Split-Path -Parent $PSScriptRoot
$previousTarget = $env:CARGO_TARGET_DIR
$sourceOptions = @()
if ($UseMirror) {
    $sourceOptions = @('--config', "source.crates-io.replace-with='rsproxy-sparse'", '--config', "source.rsproxy-sparse.registry='sparse+https://rsproxy.cn/index/'")
}
try {
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot 'target' }
    foreach ($lessonNumber in 24..26) {
        Write-Host "=== Rust lesson $lessonNumber ==="
        Push-Location (Join-Path $lessonRoot "lesson$lessonNumber")
        try {
            $testOptions = $sourceOptions + @('test', '--locked')
            if ($Offline) { $testOptions += '--offline' }
            & cargo @testOptions
            if ($LASTEXITCODE -ne 0) { throw "Rust lesson $lessonNumber tests failed" }
            $runOptions = $sourceOptions + @('run', '--locked')
            if ($Offline) { $runOptions += '--offline' }
            if ($lessonNumber -eq 26) { $runOptions += @('--', '--demo') }
            & cargo @runOptions
            if ($LASTEXITCODE -ne 0) { throw "Rust lesson $lessonNumber demo failed" }
        } finally { Pop-Location }
    }
} finally { $env:CARGO_TARGET_DIR = $previousTarget }
