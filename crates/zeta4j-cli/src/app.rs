//! Unified `z4j` command tree and dispatcher.

use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

use crate::completion::{ShellType, generate_completion_string, installation_hint};

#[derive(Parser, Debug)]
#[command(
    name = "z4j",
    version,
    about = "Unified Zeta4j client and server controller"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Manage one local Zeta4j server process.
    Server(crate::ctl::Args),
    /// Execute Cypher or open an interactive Bolt shell.
    Shell(crate::shell::Args),
    /// Administer logical databases and users over gRPC.
    Admin(crate::admin::Args),
    /// Manage the owner-only authenticated client session.
    Auth(crate::auth::Args),
    /// Generate shell completion for the unified command tree.
    Completion {
        #[arg(value_enum)]
        shell: ShellType,
        #[arg(long)]
        install: bool,
    },
    /// Print detailed version information.
    Version {
        #[arg(long)]
        json: bool,
    },
}

pub fn run() -> ExitCode {
    execute(Cli::parse())
}

/// Return every public leaf command path from the clap command tree.
///
/// The generated command tree is the source of truth; callers must not keep a
/// second hand-written command list in sync with it.
pub fn leaf_command_paths() -> Vec<Vec<String>> {
    fn collect(command: &clap::Command, parent: &[String], leaves: &mut Vec<Vec<String>>) {
        let children = command
            .get_subcommands()
            .filter(|child| child.get_name() != "help")
            .collect::<Vec<_>>();

        if children.is_empty() {
            if !parent.is_empty() {
                leaves.push(parent.to_vec());
            }
            return;
        }

        for child in children {
            let mut path = parent.to_vec();
            path.push(child.get_name().to_owned());
            collect(child, &path, leaves);
        }
    }

    let mut command = Cli::command();
    command.build();
    let mut leaves = Vec::new();
    collect(&command, &[], &mut leaves);
    leaves.sort();
    leaves
}

fn execute(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Server(args) => crate::ctl::execute(args),
        Command::Shell(args) => {
            match crate::shell::run_with_args(args, env!("CARGO_PKG_VERSION")) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Admin(args) => crate::admin::execute(args),
        Command::Auth(args) => crate::auth::execute(args),
        Command::Completion { shell, install } => {
            let mut command = Cli::command();
            if install {
                println!("{}", installation_hint(shell, "z4j"));
            } else {
                print!("{}", generate_completion_string(&mut command, shell, "z4j"));
            }
            ExitCode::SUCCESS
        }
        Command::Version { json } => crate::ctl::commands::cmd_version(json),
    }
}

#[cfg(test)]
mod tests {

    use clap::{CommandFactory, Parser, error::ErrorKind};

    use super::{Cli, leaf_command_paths};

    #[test]
    fn every_leaf_command_has_working_help() {
        let paths = leaf_command_paths();
        assert!(!paths.is_empty(), "z4j command tree must contain leaves");

        for path in paths {
            let mut argv = vec!["z4j".to_owned()];
            argv.extend(path.iter().cloned());
            argv.push("--help".to_owned());
            let error = Cli::try_parse_from(argv).err();
            assert_eq!(
                error.as_ref().map(clap::Error::kind),
                Some(ErrorKind::DisplayHelp),
                "z4j {} must render help",
                path.join(" ")
            );
        }
    }

    #[test]
    fn command_tree_has_no_compatibility_aliases() {
        fn collect(command: &clap::Command, aliases: &mut Vec<String>) {
            for child in command.get_subcommands() {
                for alias in child.get_all_aliases() {
                    aliases.push(format!("{} -> {alias}", child.get_name()));
                }
                collect(child, aliases);
            }
        }

        let mut command = Cli::command();
        command.build();
        let mut aliases = Vec::new();
        collect(&command, &mut aliases);
        assert!(
            aliases.is_empty(),
            "compatibility command aliases are forbidden: {}",
            aliases.join(", ")
        );
    }

    #[test]
    fn server_commands_have_no_edition_selection() {
        for argv in [
            vec!["z4j", "server", "init", "--edition", "base", "-y"],
            vec!["z4j", "server", "start", "--edition", "base"],
            vec!["z4j", "server", "config", "init", "--edition", "base"],
        ] {
            let error = Cli::try_parse_from(&argv).err();
            assert_eq!(
                error.as_ref().map(clap::Error::kind),
                Some(ErrorKind::UnknownArgument),
                "{} still accepts --edition",
                argv.join(" ")
            );
        }
    }

    #[test]
    fn cluster_command_is_unknown() {
        let error = Cli::try_parse_from(["z4j", "cluster", "status"]).err();
        assert_eq!(
            error.as_ref().map(clap::Error::kind),
            Some(ErrorKind::InvalidSubcommand)
        );
    }
}
