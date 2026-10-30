fn main() -> std::process::ExitCode {
    if let Err(error) = zeta4j_transport_security::install_default_crypto_provider() {
        eprintln!("Error: {error}");
        return std::process::ExitCode::FAILURE;
    }
    zeta4j_cli::app::run()
}
