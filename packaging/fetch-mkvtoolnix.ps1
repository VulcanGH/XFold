param(
    [Parameter(Mandatory)][string]$ArchiveUri,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][string]$Destination
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$archive = Join-Path $PSScriptRoot 'mkvtoolnix.7z'
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'

try {
    Invoke-WebRequest -Uri $ArchiveUri -UseBasicParsing -TimeoutSec 180 -OutFile $archive
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $Sha256) {
        throw 'The official MKVToolNix download failed its SHA-256 check.'
    }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    & $tar -xf $archive -C $Destination
    if ($LASTEXITCODE -ne 0) { throw 'Windows could not extract the MKVToolNix archive.' }
    foreach ($name in @('mkvmerge.exe', 'mkvextract.exe')) {
        if (!(Test-Path -LiteralPath (Join-Path $Destination "mkvtoolnix\$name") -PathType Leaf)) {
            throw "The MKVToolNix download is missing $name."
        }
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
