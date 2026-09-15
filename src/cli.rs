//! Maintenance commands the host runs on the server, next to the web app.

use crate::config::DEFAULT_DATABASE_URL;
use crate::db;
use crate::error::AppError;
use crate::services::AuthService;

const USAGE: &str = "Usage:
  statup                          Start the server
  statup reset-password <email>   Give an account a temporary password";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    ResetPassword(String),
}

fn parse(args: &[String]) -> Option<Command> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => Some(Command::Help),
        [name, email] if name == "reset-password" => Some(Command::ResetPassword(email.clone())),
        _ => None,
    }
}

/// Runs the command named by `args` (the process arguments without the
/// binary name) and returns the process exit code.
pub async fn run(args: &[String]) -> i32 {
    match parse(args) {
        Some(Command::Help) => {
            println!("{USAGE}");
            0
        }
        Some(Command::ResetPassword(email)) => run_reset_password(&email).await,
        None => {
            eprintln!("{USAGE}");
            2
        }
    }
}

async fn run_reset_password(email: &str) -> i32 {
    dotenvy::dotenv().ok();
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());

    match reset_password(&database_url, email).await {
        Ok(password) => {
            println!("Temporary password for {email}: {password}");
            println!(
                "Hand it over. They choose their own at next sign-in, and their open sessions are signed out."
            );
            0
        }
        Err(AppError::NotFound) => {
            eprintln!("No active account uses {email} in {database_url}.");
            1
        }
        Err(e) => {
            eprintln!("Password reset failed: {e}");
            1
        }
    }
}

async fn reset_password(database_url: &str, email: &str) -> Result<String, AppError> {
    let pool = db::create_pool(database_url, 1)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("cannot open the database: {e}")))?;
    db::run_migrations(&pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("cannot migrate the database: {e}")))?;
    AuthService::reset_password(&pool, email).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn reset_password_takes_one_email() {
        assert_eq!(
            parse(&args(&["reset-password", "a@b.com"])),
            Some(Command::ResetPassword("a@b.com".to_string()))
        );
        assert_eq!(parse(&args(&["reset-password"])), None);
    }

    #[test]
    fn unknown_commands_are_refused() {
        assert_eq!(parse(&args(&["serve"])), None);
        assert_eq!(parse(&args(&["--help"])), Some(Command::Help));
    }
}
