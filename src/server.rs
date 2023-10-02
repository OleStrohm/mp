use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

use bevy::app::ScheduleRunnerPlugin;
use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use bevy_renet::renet::{RenetServer, ServerEvent};
use bevy_renet::transport::NetcodeServerPlugin;
use bevy_renet::RenetServerPlugin;
use serde::{Deserialize, Serialize};

use crate::client::{ClientId, ClientMessage, ClientPacket, Control, Player, PlayerColor};
use crate::replicate::{
    AppExt, Channel, NetworkTick, Replicate, ReplicationConnectionConfig, ReplicationPlugin,
    PROTOCOL_ID,
};
use crate::shared::{SharedPlugin, FIXED_TIMESTEP};

pub fn server() {
    println!("Starting server!");

    App::new()
        .add_event::<ServerMessage>()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
            RenetServerPlugin,
            NetcodeServerPlugin,
            SharedPlugin,
            ReplicationPlugin,
        ))
        .replicate::<Control>()
        .replicate::<Player>()
        .replicate::<PlayerColor>()
        .replicate_with::<Transform>(
            |component| bincode::serialize(&component.translation).unwrap(),
            |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
        )
        .insert_resource(NetworkTick(0))
        .insert_resource(Lobby(Default::default()))
        .insert_resource(ServerMessages(Default::default()))
        .add_systems(Startup, start_server_networking)
        .add_systems(Update, (spawn_avatar, buffer_messages))
        .add_systems(
            FixedUpdate,
            (
                receive_client_messages,
                //send_server_message
            ),
        )
        .run();
}

fn receive_client_messages(
    mut server: ResMut<RenetServer>,
    mut players: Query<&mut Transform>,
    lobby: Res<Lobby>,
    mut message_writer: EventWriter<ServerMessage>,
) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, Channel::ReliableOrdered) {
            let packet = bincode::deserialize::<ClientPacket>(&message).unwrap();
            for message in packet.messages {
                match message {
                    ClientMessage::MoveMe(dir) => {
                        let client_entity = *lobby.get(&client_id).unwrap();
                        players.get_mut(client_entity).unwrap().translation +=
                            dir.extend(0.0) * FIXED_TIMESTEP;

                        message_writer.send(ServerMessage::MoveEntity(client_entity, dir));
                    }
                }
            }
        }
    }
}

#[derive(Debug, Resource, DerefMut, Deref)]
struct ServerMessages(Vec<ServerMessage>);

fn buffer_messages(
    mut message_reader: EventReader<ServerMessage>,
    mut buffer: ResMut<ServerMessages>,
) {
    buffer.extend(message_reader.into_iter().cloned());
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerData {
    pub network_id: Entity,
    pub pos: Vec2,
    pub color: Color,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CurrentState {
    pub players: Vec<PlayerData>,
}

#[derive(Debug, Event, Serialize, Deserialize, Clone)]
pub enum ServerMessage {
    FullSync(u64, CurrentState),
    AssignControl(u64, Entity),
    SpawnEntity(PlayerData),
    MoveEntity(Entity, Vec2),
}

#[derive(Serialize, Deserialize)]
pub struct ServerPacket {
    pub time: Duration,
    pub tick: NetworkTick,
    pub messages: Vec<ServerMessage>,
}

fn send_server_message(
    mut server: ResMut<RenetServer>,
    mut messages: ResMut<ServerMessages>,
    tick: Res<NetworkTick>,
) {
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let messages = mem::take(&mut messages.0);

    let packet = ServerPacket {
        time,
        tick: *tick,
        messages,
    };
    //println!("Sending {} messages", packet.messages.len());

    server.broadcast_message(
        Channel::ReliableOrdered,
        bincode::serialize(&packet).unwrap(),
    );
}

#[derive(Resource, Deref, DerefMut)]
pub struct Lobby(HashMap<u64, Entity>);

fn spawn_avatar(
    mut commands: Commands,
    mut lobby: ResMut<Lobby>,
    mut events: EventReader<ServerEvent>,
    mut message_writer: EventWriter<ServerMessage>,
    all_players: Query<(Entity, &Transform, &PlayerColor), With<Player>>,
) {
    let mut players_spawned_now = vec![];

    for event in &mut events {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                let color = Color::rgb(rand::random(), rand::random(), rand::random());

                let pos = 4.0 * Vec2::new(rand::random(), rand::random());

                let avatar = commands
                    .spawn((
                        Replicate,
                        Control(ClientId(*client_id)),
                        Player,
                        PlayerColor(color),
                        //Transform::default(),
                        Transform::from_translation(pos.extend(0.0)),
                    ))
                    .id();
                lobby.insert(*client_id, avatar);

                message_writer.send(ServerMessage::FullSync(
                    *client_id,
                    CurrentState {
                        players: all_players
                            .into_iter()
                            .map(|(entity, tf, color)| PlayerData {
                                network_id: entity,
                                pos: tf.translation.xy(),
                                color: color.0,
                            })
                            .chain(players_spawned_now.clone())
                            .collect(),
                    },
                ));

                let data = PlayerData {
                    network_id: avatar,
                    pos,
                    color,
                };
                players_spawned_now.push(data.clone());

                message_writer.send(ServerMessage::SpawnEntity(data));
                message_writer.send(ServerMessage::AssignControl(*client_id, avatar));

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
