#![allow(clippy::type_complexity)]

use std::ffi::OsStr;
use std::fmt::Display;
use std::process::Stdio;

use owo_colors::OwoColorize;

mod client;
mod replicate;
mod server;
mod shared;
mod player;

fn start_copy(arg: impl AsRef<OsStr>, prefix: impl Display) -> std::process::Child {
    let mut child = std::process::Command::new(std::env::args().next().unwrap())
        .arg(arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let prefix = format!("{{ print \"{} \" $0}}", prefix);

    std::process::Command::new("awk")
        .arg(prefix.clone())
        .stdin(child.stdout.take().unwrap())
        .spawn()
        .unwrap();
    std::process::Command::new("awk")
        .arg(prefix)
        .stdin(child.stderr.take().unwrap())
        .spawn()
        .unwrap();

    child
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("server") => server::server(),
        Some("client") => client::client(false),
        Some("host") | None => {
            let mut server = start_copy("server", "[Server]".green());
            let mut player2 = start_copy("client", "[P2]".yellow());

            client::client(true);

            player2.kill().unwrap();
            server.kill().unwrap();
        }
        _ => panic!("The first argument is nonsensical"),
    }
}
