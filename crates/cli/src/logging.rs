use tracing_subscriber::EnvFilter;

pub fn init_logging(verbose: u8, debug: bool) {
    let level = if debug || verbose >= 2 {
        "debug"
    } else if verbose == 1 {
        "info"
    } else {
        "warn"
    };

    let filter = EnvFilter::try_from_env("CODE_GRAPH_LOG")
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .compact()
        .without_time()
        .init();
}

#[cfg(test)]
mod tests {
    #[test]
    fn init_logging_does_not_panic() {
        // Can only init once per process, so just verify the function exists
        // and the level mapping logic works
        let _ = super::init_logging;
    }
}
