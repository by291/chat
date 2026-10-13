param(
    [Parameter(Mandatory)][string]$BackupDirectory,
    [Parameter(Mandatory)][string]$ProjectName,
    [int]$Port=55435,
    [int]$StoragePort=8338
)

$ErrorActionPreference='Stop'
$chatRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot

if($ProjectName -notmatch '^chat-restore-[a-z0-9-]+$') {
    throw 'Use a new isolated chat-restore-* project.'
}
if($Port -lt 1024 -or $Port -gt 65535 -or $StoragePort -lt 1024 -or $StoragePort -gt 65535) {
    throw 'Invalid loopback port.'
}
$backup=(Resolve-Path -LiteralPath $BackupDirectory).Path
$manifest=Get-Content -LiteralPath (Join-Path $backup 'manifest.json') -Raw | ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.database -notmatch '^[a-zA-Z0-9_]+$' -or $manifest.dbUser -notmatch '^[a-zA-Z0-9_]+$') {
    throw 'Unsupported backup manifest.'
}
foreach($name in @('postgres.dump','seaweed.tar','configuration.env','seaweed-s3.json')) {
    if((Get-FileHash -LiteralPath (Join-Path $backup $name) -Algorithm SHA256).Hash -ne $manifest.hashes.$name) {
        throw "Backup hash mismatch: $name"
    }
}

# A restore may only create a fresh project: existing state is never overwritten.
$containers=docker ps -aq --filter "label=com.docker.compose.project=$ProjectName"
if($LASTEXITCODE -ne 0){throw 'Could not inspect restore containers.'}
$volumes=docker volume ls -q --filter "label=com.docker.compose.project=$ProjectName"
if($LASTEXITCODE -ne 0){throw 'Could not inspect restore volumes.'}
if($containers -or $volumes){throw 'Restore target already exists. Choose a fresh isolated project.'}
$stateDir=Join-Path $chatRoot "backups/restore-state/$ProjectName"
if(Test-Path -LiteralPath $stateDir){throw 'Restore state directory already exists.'}
New-Item -ItemType Directory -Path $stateDir -Force | Out-Null

$values=@{}
foreach($line in Get-Content -LiteralPath (Join-Path $backup 'configuration.env')) {
    if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$') {
        $values[$Matches[1]]=$Matches[2].Trim()
    }
}
$password=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
$values.POSTGRES_USER=$manifest.dbUser
$values.POSTGRES_DB=$manifest.database
$values.POSTGRES_PASSWORD=$password
$values.POSTGRES_PORT="$Port"
$values.DATABASE_URL="postgres://$($manifest.dbUser):$password@127.0.0.1:$Port/$($manifest.database)"
$values.VALKEY_PORT='56381'
$values.VALKEY_URL='redis://127.0.0.1:56381'
$values.CENTRIFUGO_PORT='8002'
$values.CENTRIFUGO_API_URL='http://127.0.0.1:8002/api'
$values.INTERNAL_PROXY_PORT='19091'
$values.SEAWEED_S3_PORT="$StoragePort"
$values.SEAWEED_S3_CONFIG=Join-Path $backup 'seaweed-s3.json'
$values.MEDIA_S3_INTERNAL_ENDPOINT="http://127.0.0.1:$StoragePort"
$values.MEDIA_S3_PUBLIC_ENDPOINT=$values.MEDIA_S3_INTERNAL_ENDPOINT
$values.MEDIA_GATEWAY_PORT='8339'
$values.BIND_ADDR='127.0.0.1:19090'
$values.INTERNAL_BIND_ADDR='127.0.0.1:19091'
$values.WORKER_METRICS_ADDR='127.0.0.1:19092'
$values.COOKIE_SECURE='false'
$values.TRUSTED_PROXY_IPS=''

$envFile=Join-Path $stateDir 'restore.env'
$text=($values.Keys | Sort-Object | ForEach-Object {"$_=$($values[$_])"}) -join "`n"
[IO.File]::WriteAllText($envFile,$text+"`n")
# Compose prioritizes process variables over --env-file. Fence inherited values
# so they cannot redirect the restored database or its S3 configuration mount.
foreach($key in $values.Keys) {
    [Environment]::SetEnvironmentVariable($key,$values[$key],'Process')
}
$composeArgs=@('compose','--project-name',$ProjectName,'--env-file',$envFile)

& docker @composeArgs up -d --wait postgres
if($LASTEXITCODE -ne 0){throw 'Isolated PostgreSQL startup failed.'}
& docker @composeArgs cp (Join-Path $backup 'postgres.dump') 'postgres:/tmp/restore.dump'
if($LASTEXITCODE -ne 0){throw 'Restore copy failed.'}
& docker @composeArgs exec -T postgres pg_restore -U $manifest.dbUser -d $manifest.database --exit-on-error /tmp/restore.dump
if($LASTEXITCODE -ne 0){throw 'Database restore failed.'}

& docker @composeArgs create seaweedfs
if($LASTEXITCODE -ne 0){throw 'SeaweedFS volume creation failed.'}
$seaweedId=(& docker @composeArgs ps -aq seaweedfs).Trim()
if($LASTEXITCODE -ne 0 -or !$seaweedId){throw 'Could not identify the owned SeaweedFS container.'}
docker run --rm --volumes-from $seaweedId --mount "type=bind,source=$backup,target=/backup,readonly" debian:bookworm-slim tar -xf /backup/seaweed.tar -C /data
if($LASTEXITCODE -ne 0){throw 'SeaweedFS restore failed.'}

& docker @composeArgs up -d --wait --wait-timeout 60 seaweedfs valkey centrifugo
if($LASTEXITCODE -ne 0){throw 'Restored infrastructure startup failed.'}
Write-Output "Restored to isolated project $ProjectName. Local verification configuration: $envFile"
