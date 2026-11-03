# Starts the server in this folder on Windows; type its commands here, `stop` to stop it.
# RESOURCES: the game files Horizon prepares from your own Minecraft 26.3 (see README.md):
#   horizon-launcher prepare; horizon-launcher servers set-up
Set-Location $PSScriptRoot
$engine = if ($env:ENGINE) { $env:ENGINE } else { Join-Path $PSScriptRoot '..\dist\horizon-server-engine.exe' }
$resources = if ($env:RESOURCES) { $env:RESOURCES } else {
    Join-Path $env:APPDATA 'horizon-launcher\server-runtime\game-files\horizon-client\26.3'
}
if (-not (Test-Path $resources -PathType Container)) {
    Write-Error "No game files at ${resources}: set RESOURCES (see README.md)."
    exit 1
}
New-Item -ItemType Directory -Force plugins | Out-Null
& $engine --resources $resources --dedicated --nogui @args
exit $LASTEXITCODE
