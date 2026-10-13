param([switch]$IncludeIntegration)
$ErrorActionPreference = 'Stop'
$chatRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Check([string]$Name, [scriptblock]$Command) {
    Write-Host "Checking $Name"
    & $Command

    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed ($LASTEXITCODE)."
    }
}

Push-Location -LiteralPath $chatRoot
try {
    Invoke-Check 'Architecture boundaries' { node --test tests/architecture.test.mjs }
    Invoke-Check 'Rust format' { cargo fmt --all -- --check }
    Invoke-Check 'Rust lint' { cargo clippy --all-targets --all-features -- -D warnings }
    Invoke-Check 'Rust tests' { cargo test }

    Push-Location -LiteralPath (Join-Path $chatRoot 'web')
    try {
        Invoke-Check 'Svelte types' { pnpm check }
        Invoke-Check 'Web format' { pnpm lint }
        Invoke-Check 'Architecture tooling format' { pnpm exec prettier --check ../scripts/*.mjs ../tests/*.mjs }
        Invoke-Check 'API schema' { pnpm api:check }
        Invoke-Check 'Web unit tests' { pnpm test }
        Invoke-Check 'Static build' { pnpm build }

        if ($IncludeIntegration) {
            Invoke-Check 'Realtime auth' { pnpm test:realtime-auth }
            Invoke-Check 'Outbox delivery' { pnpm test:delivery }
            Invoke-Check 'Ephemeral state' { pnpm test:ephemeral }
            Invoke-Check 'Backup restore' { pnpm test:backup }
            Invoke-Check 'Browser tests' { pnpm test:e2e }
        }
    } finally {
        Pop-Location
    }
} finally {
    Pop-Location
}
