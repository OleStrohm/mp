use std::ffi::OsStr;
use std::process::Stdio;

use owo_colors::OwoColorize;

mod client;
mod server;
mod shared;

fn start_copy(arg: impl AsRef<OsStr>, prepended: String) -> std::process::Child {
    let mut child = std::process::Command::new(std::env::args().nth(0).unwrap())
        .arg(arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::process::Command::new("awk")
        .arg(prepended.clone())
        .stdin(child.stdout.take().unwrap())
        .spawn()
        .unwrap();
    std::process::Command::new("awk")
        .arg(prepended)
        .stdin(child.stderr.take().unwrap())
        .spawn()
        .unwrap();

    child
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("server") => {
            //MULTIPLAYER_ROLE.store(MultiplayerRole::Server as u8, Ordering::Relaxed);
            server::server();
        }
        Some("client") => client::client(false),
        Some("host") | None => {
            let mut server = start_copy(
                "server",
                format!("{{ print \"{} \" $0}}", "[Server]".green()),
            );
            let mut player2 =
                start_copy("client", format!("{{ print \"{} \" $0}}", "[P2]".yellow()));

            client::client(true);

            player2.kill().unwrap();
            server.kill().unwrap();
        }
        _ => panic!("The first argument is nonsensical"),
    }
}
