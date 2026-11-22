param(
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$Commit,
    [switch]$Refresh
)

$ErrorActionPreference = 'Stop'
$tag = "v$Version"
$releaseApi = "repos/$Repository/releases"

function Invoke-GitHub([string[]]$Arguments) {
    $result = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub command failed: $($Arguments[0..1] -join ' ')" }
    $result
}

function Get-Draft {
    $release = Invoke-GitHub @('release', 'view', $tag, '--repo', $Repository,
        '--json', 'databaseId,isDraft,isImmutable,targetCommitish,url') | ConvertFrom-Json
    if (!$release.isDraft -or $release.isImmutable) { throw "$tag must be an editable draft." }
    $release
}

function Get-VerifiedAssets([int64]$ReleaseId, $Expected, [string]$Prefix = '') {
    $assets = Invoke-GitHub @('api', "$releaseApi/$ReleaseId/assets", '--paginate', '--jq', '.[] | @json') |
        ForEach-Object { $_ | ConvertFrom-Json }
    foreach ($file in $Expected) {
        $name = $Prefix + $file.Name
        $asset = $assets | Where-Object name -CEQ $name
        if ($asset.state -ne 'uploaded' -or $asset.size -ne $file.Length -or $asset.digest -cne $file.Digest) {
            throw "GitHub could not verify $name."
        }
        $asset
    }
}

function Rename-Asset([int64]$Id, [string]$Name) {
    Invoke-GitHub @('api', '--method', 'PATCH', "$releaseApi/assets/$Id", '-f', "name=$Name", '--silent')
}

$prefix = "XFold-$Version"
$files = @(
    "$prefix-windows-x64-setup.exe", "$prefix-windows-x64-setup.sha256",
    "$prefix-sources.zip", "$prefix-sources.sha256"
) | ForEach-Object {
    $file = Get-Item -LiteralPath "target/release-artifacts/$_"
    [pscustomobject]@{
        Name = $file.Name
        Path = $file.FullName
        Length = $file.Length
        Digest = 'sha256:' + (Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant()
    }
}

if (!$Refresh) {
    $existing = Invoke-GitHub @('release', 'list', '--repo', $Repository, '--limit', '1000', '--json', 'tagName', '--jq', '.[].tagName')
    if ($tag -cin $existing) { throw "A draft or release already exists for $tag." }
    Invoke-GitHub (@('release', 'create', $tag) + $files.Path +
        @('--repo', $Repository, '--target', $Commit, '--draft', '--title', "XFold $Version", '--generate-notes'))
    $draft = Get-Draft
    $null = Get-VerifiedAssets $draft.databaseId $files
    return
}

$draft = Get-Draft
$refs = Invoke-GitHub @('api', "repos/$Repository/git/matching-refs/tags/$tag", '--jq', '.[].ref')
if ("refs/tags/$tag" -cin $refs) { throw "The existing $tag Git tag prevents retargeting this draft." }
$stamp = [Guid]::NewGuid().ToString('N')
$candidate = "candidate-$stamp-"
$previous = "previous-$stamp-"
$scratch = New-Item -ItemType Directory (Join-Path ([IO.Path]::GetTempPath()) "xfold-$stamp")
$uploads = @($files | ForEach-Object {
    $path = Join-Path $scratch.FullName ($candidate + $_.Name)
    Copy-Item -LiteralPath $_.Path -Destination $path
    $path
})
Invoke-GitHub (@('release', 'upload', $tag) + $uploads + @('--repo', $Repository))
$incoming = @(Get-VerifiedAssets $draft.databaseId $files $candidate)
$current = Get-Draft
if ($current.databaseId -ne $draft.databaseId -or $current.targetCommitish -cne $draft.targetCommitish) {
    throw 'The draft changed while the new files were uploading.'
}
$old = @(Invoke-GitHub @('api', "$releaseApi/$($draft.databaseId)/assets", '--paginate', '--jq', '.[] | @json') |
    ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.name -cin $files.Name })

$saved = [Collections.Generic.List[object]]::new()
$promoted = [Collections.Generic.List[object]]::new()
$retargeted = $false
try {
    foreach ($asset in $old) {
        $saved.Add($asset)
        Rename-Asset $asset.id ($previous + $asset.name)
    }
    foreach ($asset in $incoming) {
        $promoted.Add($asset)
        Rename-Asset $asset.id ($asset.name.Substring($candidate.Length))
    }
    $null = Get-VerifiedAssets $draft.databaseId $files
    $retargeted = $true
    Invoke-GitHub @('api', '--method', 'PATCH', "$releaseApi/$($draft.databaseId)",
        '-f', "target_commitish=$Commit", '--silent')
    $updated = Get-Draft
    if ($updated.targetCommitish -cne $Commit) { throw 'GitHub did not retarget the draft to the tested commit.' }
} catch {
    Write-Warning 'Draft refresh failed. Restoring the previous filenames and commit.'
    foreach ($asset in ($promoted.ToArray() + $saved.ToArray())) {
        try { Rename-Asset $asset.id $asset.name }
        catch { Write-Warning "Could not restore asset $($asset.id): $_" }
    }
    if ($retargeted) {
        try {
            Invoke-GitHub @('api', '--method', 'PATCH', "$releaseApi/$($draft.databaseId)",
                '-f', "target_commitish=$($draft.targetCommitish)", '--silent')
        } catch { Write-Warning "Could not restore the previous commit: $_" }
    }
    throw
}
foreach ($asset in $saved) {
    Invoke-GitHub @('api', '--method', 'DELETE', "$releaseApi/assets/$($asset.id)", '--silent')
}
Write-Output "Verified $($files.Count) files on draft $($updated.url)"
