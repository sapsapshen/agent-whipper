#![allow(dead_code)]

mod cli;
mod core;
mod plugins;
mod presets;
mod utils;

use clap::Parser;
use cli::commands::{Cli, Commands, ConfigAction, PresetAction};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Start { agent, mode, model } => {
            cli::commands::handle_start(&agent, &mode, model.as_deref())
        }
        Commands::Attach { session_id } => cli::commands::handle_attach(&session_id),
        Commands::Watch { all, session_id } => {
            cli::commands::handle_watch(all, session_id.as_deref())
        }
        Commands::Inject {
            session_id,
            command,
            preset,
        } => cli::commands::handle_inject(&session_id, &command, preset.as_deref()),
        Commands::Whip { preset } => cli::commands::handle_whip(&preset),
        Commands::Preset { action } => match action {
            PresetAction::List => cli::commands::handle_preset_list(),
            PresetAction::Run { name, session_id } => {
                cli::commands::handle_preset_run(&name, session_id.as_deref())
            }
            PresetAction::Create {
                name,
                description,
                trigger_on,
            } => cli::commands::handle_preset_create(&name, description.as_deref(), &trigger_on),
            PresetAction::Edit { name } => cli::commands::handle_preset_edit(&name),
        },
        Commands::Stats => cli::commands::handle_stats(),
        Commands::Status { session_id } => cli::commands::handle_status(session_id.as_deref()),
        Commands::History => cli::commands::handle_history(),
        Commands::Config { action } => match action {
            ConfigAction::Show => cli::commands::handle_config_show(),
            ConfigAction::Edit => cli::commands::handle_config_edit(),
        },
    };

    if let Err(e) = result {
        log::error!("Command failed: {}", e);
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
