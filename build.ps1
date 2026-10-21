# Compile EasyUV (application, CLI, DLL) et prépare les assets de release dans dist\.
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "échec de la compilation" }
    $py = (Get-Command py -ErrorAction SilentlyContinue) ? "py" : "python"
    & $py -3 tools\package.py @args
    if ($LASTEXITCODE -ne 0) { throw "échec du packaging" }
}
finally {
    Pop-Location
}
