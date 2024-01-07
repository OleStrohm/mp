#![allow(clippy::type_complexity)]

use std::fmt::Display;
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Stdio};
use std::time::SystemTime;

use bevy::app::AppExit;
use bevy::audio::AudioPlugin;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_renet::renet::transport::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use bevy_renet::renet::{RenetClient, RenetServer};
use owo_colors::OwoColorize;

use crate::game::GamePlugin;
use crate::player::Player;
use crate::replicate::{Owner, Replicate, PROTOCOL_ID};

use self::replicate::replication_connection_config;

mod game;
mod player;
mod prediction;
mod replicate;
#[cfg(test)]
mod test_utils;
pub mod transport;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("client") => client(
            std::env::args()
                .nth(2)
                .expect("Client needs a second argument")
                .parse::<i32>()
                .expect("Second argument must be a number"),
        ),
        Some("host") | None => {
            let client1 = start_client(1, "[C1]".green());
            let client2 = start_client(2, "[C2]".yellow());

            server(vec![client1, client2]);
        }
        _ => panic!("The first argument is nonsensical"),
    }
}

fn start_client(index: usize, prefix: impl Display) -> std::process::Child {
    let mut child = std::process::Command::new(std::env::args().next().unwrap())
        .args(["client", &format!("{index}")])
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

pub fn server(mut clients: Vec<Child>) {
    println!("Starting server!");

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;
    let window_width = monitor_width / 2.0;
    let window_height = monitor_height / 2.0;
    let position = WindowPosition::At(IVec2::new(
        (monitor_width - window_width) as i32 / 2,
        (monitor_height - window_height) as i32 / 2,
    ));
    let resolution =
        WindowResolution::new(window_width, window_height).with_scale_factor_override(1.0);

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Making a game in Rust with Bevy".to_string(),
                        position,
                        resolution: resolution.clone(),
                        resizable: false,
                        decorations: false,
                        focused: true,
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
                Transform::from_xyz(5.0, 5.0, 0.0),
                Player {
                    name: "Host".to_string(),
                    color: Color::rgb(rand::random(), rand::random(), rand::random()),
                    controller: Owner::Server,
                },
            ));
        })
        .add_systems(Last, move |app_exit: EventReader<AppExit>| {
            if !app_exit.is_empty() {
                for client in &mut clients {
                    //send_app_exit(&mut client);
                    //client.wait().unwrap();
                    client.kill().unwrap();
                }
            }
        })
        .add_systems(Update, bevy::window::close_on_esc)
        .add_systems(
            Update,
            move |mut windows: Query<&mut Window>, time: Res<Time>| {
                if time.elapsed_seconds_f64() < 1.0 {
                    for mut window in &mut windows {
                        window.position = position;
                        window.resolution = resolution.clone();
                        window.focused = true;
                    }
                }
            },
        )
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
    commands.insert_resource(Owner::Server);
}

pub fn client(index: i32) {
    println!("Starting client!");

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;
    let window_width = monitor_width / 4.0;
    let window_height = monitor_height / 4.0;
    let position = WindowPosition::At(
        (
            monitor_width as i32 / 2 - window_width as i32 * (index - 1),
            0,
        )
            .into(),
    );
    let resolution =
        WindowResolution::new(window_width, window_height).with_scale_factor_override(1.0);

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Making a game in Rust with Bevy - Client".to_string(),
                        position,
                        resolution: resolution.clone(),
                        resizable: false,
                        decorations: false,
                        focused: false,
                        ..default()
                    }),
                    ..default()
                })
                .disable::<AudioPlugin>(/* Disabled due to audio bug with pipewire */),
            //WorldInspectorPlugin::default(),
            GamePlugin,
        ))
        .add_systems(Startup, start_client_networking)
        .add_systems(
            Update,
            move |mut windows: Query<&mut Window>, time: Res<Time>| {
                if time.elapsed_seconds_f64() < 1.0 {
                    for mut window in &mut windows {
                        window.position = position;
                        window.resolution = resolution.clone();
                    }
                }
            },
        )
        .run();
}

fn start_client_networking(mut commands: Commands) {
    let client = RenetClient::new(replication_connection_config());

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let client_id = rand::random();
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
    commands.insert_resource(Owner::Client(client_id));
}
