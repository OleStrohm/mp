use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use bevy_renet::renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use bevy_renet::transport::NetcodeServerPlugin;
use bevy_renet::RenetServerPlugin;
use serde::{Deserialize, Serialize};

use crate::client::{ClientMessage, ClientPacket, Player};
use crate::shared::{NetworkTick, SharedPlugin, FIXED_TIMESTEP, PROTOCOL_ID};

pub fn server() {
    println!("Starting server!");

    App::new()
        .add_event::<ServerMessage>()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
            RenetServerPlugin,
            NetcodeServerPlugin,
            SharedPlugin,
        ))
        .insert_resource(NetworkTick(0))
        .insert_resource(Lobby(Default::default()))
        .insert_resource(ServerMessages(Default::default()))
        .add_systems(Startup, start_server_networking)
        .add_systems(Update, (spawn_avatar, buffer_messages))
        .add_systems(FixedUpdate, (send_server_message, receive_client_messages))
        .run();
}

fn receive_client_messages(
    mut server: ResMut<RenetServer>,
    mut players: Query<&mut Transform>,
    lobby: Res<Lobby>,
    mut message_writer: EventWriter<ServerMessage>,
) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            let packet = bincode::deserialize::<ClientPacket>(&message).unwrap();
            for message in packet.messages {
                //println!(
                //    "Got {message:?} messages from {client_id} which was sent at {:?}",
                //    packet.time
                //);
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
    buffer.extend(&mut message_reader);
}

#[derive(Debug, Event, Serialize, Deserialize, Clone, Copy)]
pub enum ServerMessage {
    FullSync(u64, Entity),
    AssignControl(u64, Entity),
    SpawnEntity(Entity),
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
    let messages = mem::replace(&mut messages.0, Vec::default());

    let packet = ServerPacket {
        time,
        tick: *tick,
        messages,
    };
    //println!("Sending {} messages", packet.messages.len());

    server.broadcast_message(
        DefaultChannel::ReliableOrdered,
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
) {
    for event in &mut events {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                let avatar = commands
                    .spawn((
                        Player,
                        Transform::default(),
                        //Transform::from_translation(
                        //    100.0 * Vec3::new(rand::random(), rand::random(), rand::random()),
                        //),
                    ))
                    .id();
                lobby.insert(*client_id, avatar);
                message_writer.send(ServerMessage::SpawnEntity(avatar));
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

fn start_server_networking(mut commands: Commands) {
    let server = RenetServer::new(ConnectionConfig::default());

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
