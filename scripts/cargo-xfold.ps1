param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)

$repo = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repo
try {
    $rust = Join-Path $repo 'tools/toolchain/bin'
    $kit = Join-Path $repo 'tools/build/w64devkit/bin'
    $runtime = Join-Path $repo 'tools/toolchain/lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained'
    $originalPath = $env:PATH
    $originalCargoHome = $env:CARGO_HOME
    $originalLibrary = $env:LIBRARY_PATH
    $originalFlags = $env:RUSTFLAGS
    $env:PATH = "$rust;$kit;$env:PATH"
    $env:CARGO_HOME = Join-Path $repo 'tools/cargo-home'
    $env:LIBRARY_PATH = $runtime
    $env:RUSTFLAGS = '-C link-self-contained=yes'
    & (Join-Path $rust 'cargo.exe') @CargoArgs
    exit $LASTEXITCODE
} finally {
    $env:PATH = $originalPath
    $env:CARGO_HOME = $originalCargoHome
    $env:LIBRARY_PATH = $originalLibrary
    $env:RUSTFLAGS = $originalFlags
    Pop-Location
}
