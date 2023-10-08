#![allow(clippy::type_complexity)]

use std::ffi::OsStr;
use std::fmt::Display;
use std::net::{SocketAddr, UdpSocket};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use bevy::app::ScheduleRunnerPlugin;
use bevy::audio::AudioPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy_renet::renet::transport::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use bevy_renet::renet::{RenetClient, RenetServer};
use owo_colors::OwoColorize;

use crate::game::GamePlugin;
use crate::replicate::{ClientId, ReplicationConnectionConfig, PROTOCOL_ID};

//mod client;
mod game;
mod player;
mod prediction;
mod replicate;
//mod server;
mod shared;
pub mod transport;

static HOST: AtomicBool = AtomicBool::new(false);

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
        Some("server") => server(),
        Some("client") => client(false),
        Some("host") | None => {
            let mut server = start_copy("server", "[Server]".green());
            let mut player2 = start_copy("client", "[P2]".yellow());

            client(true);

            player2.kill().unwrap();
            server.kill().unwrap();
        }
        _ => panic!("The first argument is nonsensical"),
    }
}

pub fn server() {
    println!("Starting server!");

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
            InputPlugin,
            GamePlugin,
        ))
        .add_systems(Startup, start_server_networking)
        .run();
}

fn start_server_networking(
    mut commands: Commands,
    connection_config: Res<ReplicationConnectionConfig>,
) {
    let server = RenetServer::new(connection_config.0.clone());

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let public_addr = "127.0.0.1:5000".parse::<SocketAddr>().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let server_config = ServerConfig {
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        authentication: ServerAuthentication::Unsecure,
        public_addr,
    };

    let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

    commands.insert_resource(transport);
    commands.insert_resource(server);
}

pub fn client(main: bool) {
    println!("Starting client!");
    HOST.store(main, Ordering::Relaxed);

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: if main {
                            "Making a game in Rust with Bevy".to_string()
                        } else {
                            "Making a game in Rust with Bevy - player 2".to_string()
                        },
                        position: if main {
                            WindowPosition::Centered(MonitorSelection::Primary)
                        } else {
                            WindowPosition::At((10, 10).into())
                        },
                        resolution: if main {
                            (monitor_width / 2.0, monitor_height / 2.0).into()
                        } else {
                            (monitor_width / 4.75, monitor_height / 4.75).into()
                        },
                        resizable: false,
                        decorations: false,
                        ..default()
                    }),
                    ..default()
                })
                .disable::<AudioPlugin>(/* Disabled due to audio bug with pipewire */),
            GamePlugin,
        ))
        .add_systems(Startup, start_client_networking)
        .run();
}

fn start_client_networking(
    mut commands: Commands,
    connection_config: Res<ReplicationConnectionConfig>,
) {
    let client = RenetClient::new(connection_config.0.clone());

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let client_id = if HOST.load(Ordering::Relaxed) {
        0
    } else {
        rand::random()
    };
    let server_addr = "127.0.0.1:5000".parse::<SocketAddr>().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr,
        user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    commands.insert_resource(transport);
    commands.insert_resource(client);
    commands.insert_resource(ClientId(client_id));
}
