//! `powerz` binary: parses arguments and hands off to [`powerz_cli::run`].

use clap::Parser;

fn main() {
    let cli = powerz_cli::Cli::parse();
    std::process::exit(powerz_cli::run(cli));
}
