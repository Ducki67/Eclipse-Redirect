Set-Location $PSScriptRoot



cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "build failed" -ForegroundColor Red
    pause
    exit $LASTEXITCODE
}

$src = "target\release\eclipse_redirect.dll"
$out = "out\release"
$dst = "$out\Eclipse Redirect.dll"

if (-not (Test-Path $src)) {
    Write-Host ""
    Write-Host "missing $src" -ForegroundColor Red
    pause
    exit 1
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
Get-ChildItem $out -Filter *.dll -ErrorAction SilentlyContinue | Remove-Item -Force
Copy-Item $src $dst -Force

$size = (Get-Item $dst).Length
Write-Host ""
Write-Host "built -> $dst ($size bytes)" -ForegroundColor Green
pause
