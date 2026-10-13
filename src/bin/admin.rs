//! Operator CLI; passwords are collected without echo and never passed as arguments.

use chat_server::{
    AppState,
    auth::{
        password::{MAX_PASSWORD_CHARACTERS, MIN_PASSWORD_CHARACTERS},
        service,
    },
    infrastructure::{config::Config, db::connect_db},
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Self-hosted Chat administration; passwords are entered without echo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    MediaInit,
    MediaCleanupTest,
    Bootstrap {
        #[arg(long)]
        username: String,
        #[arg(long)]
        display_name: String,
    },
    Invite,
    ResetPassword {
        #[arg(long)]
        username: String,
    },
}

fn password() -> anyhow::Result<String> {
    // The hidden-input library writes prompt bytes directly to the console. ASCII
    // operator prompts also work when the Windows console uses a legacy code page.
    let prompt =
        format!("Password ({MIN_PASSWORD_CHARACTERS}-{MAX_PASSWORD_CHARACTERS} characters): ");
    let first = rpassword::prompt_password(prompt)?;
    let second = rpassword::prompt_password("Confirm password: ")?;

    anyhow::ensure!(first == second, "Passwords do not match");
    Ok(first)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::from_env()?;
    if matches!(cli.command, Command::MediaInit | Command::MediaCleanupTest) {
        let storage = config
            .media_storage
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Media credentials are not configured"))?;
        let storage = chat_server::infrastructure::object_storage::ObjectStorage::new(storage);
        if matches!(cli.command, Command::MediaCleanupTest) {
            storage.destroy_browser_test_bucket().await?;
            println!("Owned browser-test bucket removed.");
        } else {
            storage.ensure_bucket().await?;
            println!("Media bucket is ready.");
        }
        return Ok(());
    }
    let pool = connect_db(&config).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let state = AppState::new(config, pool).await?;

    match cli.command {
        Command::MediaInit => unreachable!("media initialization returns before database setup"),
        Command::MediaCleanupTest => unreachable!("test cleanup returns before database setup"),
        Command::Bootstrap {
            username,
            display_name,
        } => {
            let password = password()?;
            let user = service::bootstrap(&state, &username, &display_name, &password).await?;
            println!("Created initial user @{}", user.username);
        }
        Command::Invite => {
            let token = service::create_invitation(&state).await?;
            println!("Invitation (single use, expires in 7 days): {token}");
        }
        Command::ResetPassword { username } => {
            let password = password()?;
            service::reset_password(&state, &username, &password).await?;
            println!("Password reset; all device sessions revoked.");
        }
    }
    Ok(())
}
