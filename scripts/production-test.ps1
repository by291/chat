param([switch]$Prepare,[switch]$Cleanup)
$ErrorActionPreference='Stop'
$chatRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot
$scratch=Join-Path $chatRoot '.superpowers/sdd/2026-10-12-phase-1-private-chat/deployment'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$envFile=Join-Path $scratch 'test.env'
if($Cleanup){
    if(Test-Path -LiteralPath $envFile){docker compose --project-name chat-deployment-test --env-file $envFile --file infra/compose/compose.prod.yaml --file infra/compose/compose.deployment-test.yaml down; if($LASTEXITCODE -ne 0){throw 'Deployment test shutdown failed.'}}
    exit 0
}
if(!$Prepare){throw 'Use -Prepare or -Cleanup.'}
$text=Get-Content -LiteralPath (Join-Path $chatRoot '.env') -Raw
$text += "`nCHAT_DOMAIN=https://localhost`nCHAT_ORIGIN=https://localhost:5443`nCHAT_SUBNET=172.30.241.0/24`nCHAT_CADDY_IP=172.30.241.10`n"
[IO.File]::WriteAllText($envFile,$text)
docker compose --project-name chat-deployment-test --env-file $envFile --file infra/compose/compose.prod.yaml --file infra/compose/compose.deployment-test.yaml up -d --build --wait --wait-timeout 240
if($LASTEXITCODE -ne 0){throw 'Deployment test startup failed.'}
Write-Output 'Deployment test available at https://localhost:5443 (test-only internal CA).'
