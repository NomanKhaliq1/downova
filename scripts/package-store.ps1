# Packs the extension for the Chrome Web Store / Edge Add-ons: dist\store\<product>-extension-<version>.zip
# The store assigns the extension ID itself, so the "key" line of the manifest is removed from this copy.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
& powershell -NoProfile -File "$root\scripts\package-extension.ps1"
if ($LASTEXITCODE -ne 0) { throw 'extension packaging failed' }
$m = Get-Content "$root\dist\extension\manifest.json" -Raw | ConvertFrom-Json
$product = [regex]::Match((Get-Content "$root\brand\brand.toml" -Raw), '(?m)^product_id\s*=\s*"([^"]+)"').Groups[1].Value
$stage = Join-Path $root 'dist\store-stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
Copy-Item "$root\dist\extension" $stage -Recurse
$sm = Get-Content "$stage\manifest.json" -Raw | ConvertFrom-Json
$sm.PSObject.Properties.Remove('key')
[IO.File]::WriteAllText("$stage\manifest.json", ($sm | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
New-Item -ItemType Directory -Force "$root\dist\store" | Out-Null
$zip = "$root\dist\store\$product-extension-$($m.version).zip"
if (Test-Path $zip) { Remove-Item $zip }
Add-Type -AssemblyName System.IO.Compression.FileSystem
# entry names must use forward slashes (Windows PowerShell's own zip helpers write backslashes, which the store rejects)
$za = [IO.Compression.ZipFile]::Open($zip, 'Create')
try {
  Get-ChildItem $stage -Recurse -File | ForEach-Object {
    $name = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($za, $_.FullName, $name, 'Optimal')
  }
} finally { $za.Dispose() }
Remove-Item $stage -Recurse -Force
Write-Host ("{0}  {1:N1} KB" -f $zip, ((Get-Item $zip).Length / 1KB))
