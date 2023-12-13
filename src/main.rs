#![allow(clippy::type_complexity)]

use std::ffi::OsStr;
use std::fmt::Display;
use std::io::{stdin, Write};
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::SystemTime;

use bevy::app::AppExit;
use bevy::audio::AudioPlugin;
use bevy::prelude::*;
use bevy::utils::synccell::SyncCell;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_renet::renet::transport::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use bevy_renet::renet::{RenetClient, RenetServer};
use bevy_xpbd_2d::plugins::PhysicsDebugPlugin;
use owo_colors::OwoColorize;

use crate::game::GamePlugin;
use crate::player::Player;
use crate::replicate::{Owner, PROTOCOL_ID, Replicate};

use self::replicate::replication_connection_config;

mod game;
mod player;
mod prediction;
mod replicate;
#[cfg(test)]
mod test_utils;
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

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;

    fn fix_window_pos(mut windows: Query<&mut Window>) {
        for mut window in &mut windows {
            window.position = WindowPosition::Centered(MonitorSelection::Primary);
        }
    }

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Making a game in Rust with Bevy - Server".to_string(),
                        position: WindowPosition::Centered(MonitorSelection::Primary),
                        resolution: (monitor_width / 2.0, monitor_height / 2.0).into(),
                        resizable: false,
                        decorations: false,
                        ..default()
                    }),
                    ..default()
                })
                .disable::<AudioPlugin>(/* Disabled due to audio bug with pipewire */),
            WorldInspectorPlugin::default(),
            GamePlugin,
        ))
        .add_systems(Startup, start_server_networking)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn((
                Replicate,
                Player {
                    color: Color::rgb(rand::random(), rand::random(), rand::random()),
                    controller: Owner(1),
                },
                Transform::from_xyz(5.0, 5.0, 0.0),
            ));
        })
        .add_systems(
            Update,
            fix_window_pos.run_if(any_with_component::<Window>().and_then(run_once())),
        )
        .add_systems(Update, move |mut app_exit: EventWriter<AppExit>| {
            if let Ok("exit") = receiver.get().try_recv().as_deref() {
                app_exit.send(AppExit);
            }
        })
        .run();
}

fn start_server_networking(mut commands: Commands) {
    let server = RenetServer::new(replication_connection_config());

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let public_addr = "127.0.0.1:5000".parse::<SocketAddr>().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let server_config = ServerConfig {
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        authentication: ServerAuthentication::Unsecure,
        current_time,
        public_addresses: vec![public_addr],
    };

    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();

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
                            WindowPosition::At((10, 10+ (monitor_height / 4.75) as i32).into())
                        } else {
                            WindowPosition::At((10, 10).into())
                        },
                        resolution: (monitor_width / 4.75, monitor_height / 4.75).into(),
                        resizable: false,
                        decorations: false,
                        ..default()
                    }),
                    ..default()
                })
                .disable::<AudioPlugin>(/* Disabled due to audio bug with pipewire */),
            GamePlugin,
            PhysicsDebugPlugin::default(),
        ))
        .add_systems(Startup, start_client_networking)
        .add_systems(Update, move |mut app_exit: EventWriter<AppExit>| {
            if let Ok("exit") = receiver.get().try_recv().as_deref() {
                app_exit.send(AppExit);
            }
        })
        //.add_systems(
        //    Update,
        //    fix_window_pos.run_if(any_with_component::<Window>().and_then(run_once())),
        //)
        .add_systems(Last, move |app_exit: EventReader<AppExit>| {
            if !app_exit.is_empty() {
                let Some((player2, server)) = children.as_mut() else {
                    return;
                };

                send_app_exit(player2);
                player2.wait().unwrap();
                send_app_exit(server);
                server.wait().unwrap();
            }
        })
        .run();
}

fn start_client_networking(mut commands: Commands) {
    let client = RenetClient::new(replication_connection_config());

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
    commands.insert_resource(Owner(client_id));
}
