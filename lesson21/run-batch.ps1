param([switch]$Bench)
$ErrorActionPreference = 'Stop'
$lessonRoot = Split-Path -Parent $PSScriptRoot
foreach ($lessonNumber in 21..23) {
    Write-Host "=== Rust lesson $lessonNumber ==="
    Push-Location (Join-Path $lessonRoot "lesson$lessonNumber")
    try {
        cargo test --offline --locked
        if ($LASTEXITCODE -ne 0) { throw "Rust lesson $lessonNumber tests failed" }
        cargo run --offline --locked
        if ($LASTEXITCODE -ne 0) { throw "Rust lesson $lessonNumber demo failed" }
    } finally { Pop-Location }
}
if ($Bench) {
    Push-Location (Join-Path $lessonRoot 'lesson22')
    try {
        cargo bench --offline --locked --bench membership
        if ($LASTEXITCODE -ne 0) { throw 'Rust benchmarks failed' }
    } finally { Pop-Location }
}
