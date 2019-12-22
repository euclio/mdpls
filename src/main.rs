use std::env;
use std::error::Error;
use std::io;

use mdpls::Server;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut server = Server::new(stdin.lock(), stdout.lock());
    server.test = env::args().any(|arg| arg.contains("test"));
    server.serve()?;

    Ok(())
}
