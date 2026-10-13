# Fails (exit 1) when an .exe or .dll under -Path carries no Authenticode signature, or, with -Publisher, one signed by
# anyone else. Test-signed files report UnknownError/NotTrusted and still count as signed. A file changed after signing
# (HashMismatch) fails too, and so does a scan that finds no .exe or .dll at all.
# Works in Windows PowerShell 5.1 and PowerShell 7.
param(
  [Parameter(Mandatory = $true)][string[]]$Path,
  [string]$Publisher
)
$ErrorActionPreference = 'Stop'

function Get-SignerCommonName([string]$Subject) {
  if ($Subject -match '(?:^|,\s*)CN=(?:"([^"]+)"|([^,]+))') {
    if ($Matches[1]) { return $Matches[1] }
    return $Matches[2].Trim()
  }
  return $null
}

# `powershell -File x.ps1 -Path a,b` hands over the comma list as one string, so split it unless it names a real path.
$paths = @()
foreach ($p in $Path) {
  if ((-not (Test-Path -LiteralPath $p)) -and $p.Contains(',')) { $paths += $p.Split(',') | ForEach-Object { $_.Trim() } } else { $paths += $p }
}

$files = @()
foreach ($p in $paths) {
  if (-not (Test-Path -LiteralPath $p)) { Write-Host "missing: $p"; exit 1 }
  $item = Get-Item -LiteralPath $p
  if ($item.PSIsContainer) {
    $files += Get-ChildItem -LiteralPath $p -Recurse -File | Where-Object { $_.Extension -in '.exe', '.dll' }
  } else {
    $files += $item
  }
}

if ($files.Count -eq 0) { Write-Host "no .exe or .dll files found under: $($paths -join ', ')"; exit 1 }

$offenders = @()
foreach ($f in $files) {
  $sig = Get-AuthenticodeSignature -LiteralPath $f.FullName
  if ($sig.Status -eq 'NotSigned' -or $null -eq $sig.SignerCertificate) {
    $offenders += "unsigned: $($f.FullName)"
    continue
  }
  if ($sig.Status -eq 'HashMismatch') {
    $offenders += "modified after signing (HashMismatch): $($f.FullName)"
    continue
  }
  if ($Publisher) {
    $cn = Get-SignerCommonName $sig.SignerCertificate.Subject
    if ($cn -ne $Publisher) { $offenders += "publisher '$cn' is not '$Publisher': $($f.FullName)" }
  }
}

if ($offenders.Count -gt 0) {
  $offenders | ForEach-Object { Write-Host $_ }
  Write-Host "FAILED: $($offenders.Count) of $($files.Count) files"
  exit 1
}
Write-Host "OK: $($files.Count) files signed$(if ($Publisher) { " by $Publisher" })"
exit 0
