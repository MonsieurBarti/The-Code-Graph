use clap::Parser;
use cli::commands::Cli;
use cli::logging::init_logging;

fn main() {
    let cli = Cli::parse();

    init_logging(cli.verbose, cli.debug);

    let exit_code = match cli::run(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            exit_code_for(&e)
        }
    };

    std::process::exit(exit_code);
}

fn exit_code_for(err: &domain::error::CodeGraphError) -> i32 {
    match err {
        domain::error::CodeGraphError::NoProject => 2,
        domain::error::CodeGraphError::BlocklistedRoot(_) => 2,
        _ => 1,
    }
}
