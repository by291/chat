param([Parameter(Mandatory)][string]$BackupDirectory,[Parameter(Mandatory)][string]$ProjectName,[int]$Port=55435)
$ErrorActionPreference='Stop'
$chatRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot
if($ProjectName -notmatch '^chat-restore-[a-z0-9-]+$'){throw 'Use a new isolated chat-restore-* project.'}
if($Port -lt 1024 -or $Port -gt 65535){throw 'Invalid loopback port.'}
$backup=(Resolve-Path -LiteralPath $BackupDirectory).Path
$manifest=Get-Content -LiteralPath (Join-Path $backup 'manifest.json') -Raw|ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.database -notmatch '^[a-zA-Z0-9_]+$' -or $manifest.dbUser -notmatch '^[a-zA-Z0-9_]+$'){throw 'Unsupported backup manifest.'}
foreach($name in @('postgres.dump','seaweed.tar','configuration.env','seaweed-s3.json')){if((Get-FileHash -LiteralPath (Join-Path $backup $name) -Algorithm SHA256).Hash -ne $manifest.hashes.$name){throw "Backup hash mismatch: $name"}}
$containers=docker ps -aq --filter "label=com.docker.compose.project=$ProjectName"
$volumes=docker volume ls -q --filter "label=com.docker.compose.project=$ProjectName"
if($containers -or $volumes){throw 'Restore target already exists. Choose a fresh isolated project; existing state is never overwritten.'}
$stateDir=Join-Path $chatRoot "backups/restore-state/$ProjectName"
if(Test-Path -LiteralPath $stateDir){throw 'Restore state directory already exists.'}
New-Item -ItemType Directory -Path $stateDir -Force|Out-Null
$password=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
$envFile=Join-Path $stateDir 'restore.env'
[IO.File]::WriteAllText($envFile,"RESTORE_DB_USER=$($manifest.dbUser)`nRESTORE_DB_NAME=$($manifest.database)`nRESTORE_PASSWORD=$password`nRESTORE_PORT=$Port`nRESTORE_S3_CONFIG=$(Join-Path $backup 'seaweed-s3.json')`n")
$composeArgs=@('compose','--project-name',$ProjectName,'--env-file',$envFile,'--file','infra/compose/compose.restore.yaml')
& docker @composeArgs up -d --wait postgres;if($LASTEXITCODE -ne 0){throw 'Isolated PostgreSQL startup failed.'}
& docker @composeArgs cp (Join-Path $backup 'postgres.dump') 'postgres:/tmp/restore.dump';if($LASTEXITCODE -ne 0){throw 'Restore copy failed.'}
& docker @composeArgs exec -T postgres pg_restore -U $manifest.dbUser -d $manifest.database --exit-on-error /tmp/restore.dump;if($LASTEXITCODE -ne 0){throw 'Database restore failed.'}
& docker @composeArgs create seaweedfs;if($LASTEXITCODE -ne 0){throw 'SeaweedFS volume creation failed.'}
$seaweedId=(& docker @composeArgs ps -aq seaweedfs).Trim()
docker run --rm --volumes-from $seaweedId --mount "type=bind,source=$backup,target=/backup,readonly" debian:bookworm-slim tar -xf /backup/seaweed.tar -C /data
if($LASTEXITCODE -ne 0){throw 'SeaweedFS restore failed.'}
& docker @composeArgs up -d seaweedfs;if($LASTEXITCODE -ne 0){throw 'SeaweedFS restore startup failed.'}
Write-Output "Restored to isolated project $ProjectName. Local verification configuration: $envFile"
