//! Operator CLI; passwords are collected without echo and never passed as arguments.

use chat_server::{
    AppState,
    auth::service,
    infrastructure::{config::Config, db::connect_db},
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Chat 自托管管理；密码通过无回显提示输入")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    let first = rpassword::prompt_password("密码 (12–128 个字符): ")?;
    let second = rpassword::prompt_password("再次输入: ")?;
    anyhow::ensure!(first == second, "两次密码不一致");
    Ok(first)
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::from_env()?;
    let pool = connect_db(&config).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let state = AppState::new(config, pool).await?;

    match cli.command {
        Command::Bootstrap {
            username,
            display_name,
        } => {
            let password = password()?;
            let user = service::bootstrap(&state, &username, &display_name, &password).await?;
            println!("已创建管理员 @{}", user.username);
        }
        Command::Invite => {
            let token = service::create_invitation(&state).await?;
            println!("邀请（7 天内单次有效）: {token}");
        }
        Command::ResetPassword { username } => {
            let password = password()?;
            service::reset_password(&state, &username, &password).await?;
            println!("已重置密码并撤销全部设备会话");
        }
    }
    Ok(())
}
