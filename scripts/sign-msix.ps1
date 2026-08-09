param(
    [Parameter(Mandatory)][string]$Package,
    [Parameter(Mandatory)][string]$PfxPath,
    [Parameter(Mandatory)][securestring]$Password
)

$ErrorActionPreference = "Stop"
$packagePath = (Resolve-Path -LiteralPath $Package).Path
$certificatePath = (Resolve-Path -LiteralPath $PfxPath).Path
$signTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe |
    Where-Object FullName -Match '\\x64\\signtool\.exe$' | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signTool) { throw "Windows SDK signtool.exe was not found" }
$credential = [System.Net.NetworkCredential]::new('', $Password)
try {
    & $signTool.FullName sign /fd SHA256 /f $certificatePath /p $credential.Password $packagePath
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }
} finally {
    $credential.Password = ""
}
$signature = Get-AuthenticodeSignature -LiteralPath $packagePath
if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
    throw "Signed package did not validate: $($signature.Status)"
}
Write-Output "Signed MSIX: $packagePath"
