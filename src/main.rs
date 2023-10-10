#![allow(clippy::type_complexity)]

use std::ffi::OsStr;
use std::fmt::Display;
use std::io::{stdin, Write};
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::audio::AudioPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::utils::synccell::SyncCell;
use bevy_renet::renet::transport::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use bevy_renet::renet::{RenetClient, RenetServer};
use owo_colors::OwoColorize;

use crate::game::GamePlugin;
use crate::replicate::{ClientId, ReplicationConnectionConfig, PROTOCOL_ID};

mod game;
mod player;
mod prediction;
mod replicate;
pub mod transport;

static HOST: AtomicBool = AtomicBool::new(false);

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("server") => server(),
        Some("client") => client(false, None),
        Some("host") | None => {
            let server = start_copy("server", "[Server]".green());
            let player2 = start_copy("client", "[P2]".yellow());

            client(true, Some((player2, server)));
        }
        _ => panic!("The first argument is nonsensical"),
    }
}

fn start_copy(arg: impl AsRef<OsStr>, prefix: impl Display) -> std::process::Child {
    let mut child = std::process::Command::new(std::env::args().next().unwrap())
        .arg(arg)
        .stdin(Stdio::piped())
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

fn send_app_exit(child: &mut Child) {
    write!(child.stdin.as_mut().unwrap(), "exit").unwrap()
}

pub fn server() {
    println!("Starting server!");

    let (sender, receiver) = mpsc::channel();
    let mut receiver = SyncCell::new(receiver);

    std::thread::spawn(move || {
        let mut buffer = String::new();
        stdin().read_line(&mut buffer).unwrap();
        sender.send(buffer).unwrap();
    });

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
            InputPlugin,
            GamePlugin,
        ))
        .add_systems(Startup, start_server_networking)
        .add_systems(Update, move |mut app_exit: EventWriter<AppExit>| {
            if let Ok("exit") = receiver.get().try_recv().as_deref() {
                app_exit.send(AppExit);
            }
        })
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

pub fn client(main: bool, mut children: Option<(Child, Child)>) {
    println!("Starting client!");
    HOST.store(main, Ordering::Relaxed);

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;

    let (sender, receiver) = mpsc::channel();
    let mut receiver = SyncCell::new(receiver);

    std::thread::spawn(move || {
        let mut buffer = String::new();
        stdin().read_line(&mut buffer).unwrap();
        sender.send(buffer).unwrap();
    });

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
        .add_systems(Update, move |mut app_exit: EventWriter<AppExit>| {
            if let Ok("exit") = receiver.get().try_recv().as_deref() {
                app_exit.send(AppExit);
            }
        })
        .add_systems(Last, move |app_exit: EventReader<AppExit>| {
            if !app_exit.is_empty() {
                let Some((player2, server)) = children.as_mut() else { return };

                send_app_exit(player2);
                player2.wait().unwrap();
                send_app_exit(server);
                server.wait().unwrap();
            }
        })
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
