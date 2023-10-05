use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use bevy_renet::renet::{RenetServer, ServerEvent};
use bevy_renet::transport::NetcodeServerPlugin;
use bevy_renet::RenetServerPlugin;

use crate::client::ClientPacket;
use crate::player::{Player, PlayerPlugin};
use crate::replicate::schedule::NetworkPreUpdate;
use crate::replicate::{
    copy_input_from_history, Channel, ClientId, Replicate, ReplicationConnectionConfig,
    ReplicationPlugin, PROTOCOL_ID,
};
use crate::shared::{SharedPlugin, FIXED_TIMESTEP};

#[derive(Resource, Deref, DerefMut, Default)]
pub struct Lobby(HashMap<u64, Entity>);

pub fn server() {
    println!("Starting server!");

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
            RenetServerPlugin,
            NetcodeServerPlugin,
            SharedPlugin,
            ReplicationPlugin::with_step(FIXED_TIMESTEP),
            PlayerPlugin,
        ))
        .init_resource::<Lobby>()
        .add_systems(Startup, start_server_networking)
        .add_systems(Update, spawn_avatar)
        .add_systems(
            NetworkPreUpdate,
            (
                receive_client_messages.run_if(is_server),
                apply_deferred,
                copy_input_from_history,
                apply_deferred,
            )
                .chain(),
        )
        .run();
}

fn is_server(server: Option<Res<RenetServer>>) -> bool {
    server.is_some()
}

fn receive_client_messages(
    mut commands: Commands,
    mut server: ResMut<RenetServer>,
    lobby: Res<Lobby>,
) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, Channel::ReliableOrdered) {
            let packet = bincode::deserialize::<ClientPacket>(&message).unwrap();
            let history = packet.history;
            let client_entity = *lobby.get(&client_id).unwrap();

            commands.entity(client_entity).insert(history);
        }
    }
}

fn spawn_avatar(
    mut commands: Commands,
    mut lobby: ResMut<Lobby>,
    mut events: EventReader<ServerEvent>,
) {
    for event in &mut events {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                let color = Color::rgb(rand::random(), rand::random(), rand::random());
                let pos = 4.0 * Vec2::new(rand::random(), rand::random());

                let avatar = commands
                    .spawn((
                        Replicate,
                        Player {
                            color,
                            controller: ClientId(*client_id),
                        },
                        Transform::from_translation(pos.extend(0.0)),
                    ))
                    .id();
                lobby.insert(*client_id, avatar);

                println!(
                    "{} connected! Creating the avatar as {:?}",
                    *client_id, avatar
                );
            }
            ServerEvent::ClientDisconnected {
                client_id: _client_id,
                reason: _reason,
            } => {
                println!("{} disconnected ({})", _client_id, _reason);
            }
        }
    }
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
