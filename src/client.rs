use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::{ClientAuthentication, NetcodeClientTransport};
use bevy_renet::renet::{ConnectionConfig, DefaultChannel, RenetClient};
use bevy_renet::transport::{client_connected, NetcodeClientPlugin};
use bevy_renet::RenetClientPlugin;
use serde::{Deserialize, Serialize};

use crate::server::{ServerMessage, ServerPacket};
use crate::shared::{NetworkTick, SharedPlugin, PROTOCOL_ID, FIXED_TIMESTEP};

static HOST: AtomicBool = AtomicBool::new(false);

#[derive(Resource, Deref, DerefMut)]
pub struct NetworkEntities(HashMap<Entity, Entity>);

pub fn client(main: bool) {
    println!("Starting client!");
    HOST.store(main, Ordering::Relaxed);

    let monitor_width = 2560.0;
    let monitor_height = 1440.0;

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
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
            }),
            RenetClientPlugin,
            NetcodeClientPlugin,
            SharedPlugin,
        ))
        .add_event::<ClientMessage>()
        .insert_resource(NetworkEntities(Default::default()))
        .add_systems(Startup, (startup, start_client_networking))
        .add_systems(
            Update,
            (
                add_visual_for_other_players,
                add_visual_for_controlled_character,
                send_client_messages.run_if(client_connected()),
            ),
        )
        .add_systems(FixedUpdate, move_player)
        .add_systems(Update, receive_server_messages)
        .run();
}

#[derive(Event, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ClientMessage {
    MoveMe(Vec2),
}

#[derive(Serialize, Deserialize)]
pub struct ClientPacket {
    pub time: Duration,
    pub tick: NetworkTick,
    pub messages: Vec<ClientMessage>,
}

fn send_client_messages(
    mut messsage_reader: EventReader<ClientMessage>,
    mut client: ResMut<RenetClient>,
) {
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let messages = messsage_reader.into_iter().cloned().collect();

    let packet = ClientPacket {
        time,
        tick: NetworkTick(0),
        messages,
    };
    //println!("Sending {} messages", packet.messages.len());

    client.send_message(
        DefaultChannel::ReliableOrdered,
        bincode::serialize(&packet).unwrap(),
    );
}

fn receive_server_messages(
    mut commands: Commands,
    mut network_entities: ResMut<NetworkEntities>,
    mut client: ResMut<RenetClient>,
    tick: Option<Res<NetworkTick>>,
    mut players: Query<&mut Transform>,
) {
    let Some(packet) = client.receive_message(DefaultChannel::ReliableOrdered) else { return };
    let packet: ServerPacket = bincode::deserialize(&packet).unwrap();

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    //println!(
    //    "Received {} message(s) at Server tick: {:?} (Client {:?})    |     {:?}",
    //    packet.messages.len(),
    //    packet.tick.0,
    //    tick,
    //    time - packet.time,
    //);
    let this_client_id = HOST.load(Ordering::Relaxed) as u64; //current_time.as_millis() as u64;
    for message in packet.messages {
        match message {
            ServerMessage::FullSync(client_id, _state) => {
                println!("Fully synced client {client_id}")
            }
            ServerMessage::AssignControl(client_id, network_id) => {
                if client_id == this_client_id {
                    let local_entity = *network_entities.get(&network_id).unwrap();
                    commands.entity(local_entity).insert(Control);
                }
            }
            ServerMessage::SpawnEntity(network_id) => {
                let spawned = commands.spawn(Player).id();
                network_entities.insert(network_id, spawned);
            }
            ServerMessage::MoveEntity(network_id, dir) => {
                let local_entity = *network_entities.get(&network_id).unwrap();
                players.get_mut(local_entity).unwrap().translation += dir.extend(0.0) * FIXED_TIMESTEP;
            }
        }
    }
}

#[derive(Component, Serialize, Deserialize)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player;

fn add_visual_for_controlled_character(
    mut commands: Commands,
    mut controlled: Query<(Entity, Option<&mut Sprite>), Added<Control>>,
) {
    for (controlled, sprite) in &mut controlled {
        if let Some(mut sprite) = sprite {
            sprite.color = Color::GREEN;
        } else {
            commands.entity(controlled).insert(SpriteBundle {
                sprite: Sprite {
                    color: Color::GREEN,
                    custom_size: Some(Vec2::splat(100.0)),
                    ..default()
                },
                ..default()
            });
        }
    }
}

fn add_visual_for_other_players(
    mut commands: Commands,
    others: Query<Entity, (Added<Player>, Without<Sprite>)>,
) {
    for other in &others {
        commands.entity(other).insert(SpriteBundle {
            sprite: Sprite {
                color: Color::GRAY,
                custom_size: Some(Vec2::splat(100.0)),
                ..default()
            },
            ..default()
        });
    }
}

fn move_player(
    mut query: Query<&Transform, With<Control>>,
    input: Res<Input<KeyCode>>,
    //time: Res<Time>,
    mut message: EventWriter<ClientMessage>,
) {
    for _tf in &mut query {
        let mut dir = Vec3::splat(0.0);
        if input.pressed(KeyCode::W) {
            dir += Vec3::Y;
        }
        if input.pressed(KeyCode::S) {
            dir -= Vec3::Y;
        }
        if input.pressed(KeyCode::A) {
            dir -= Vec3::X;
        }
        if input.pressed(KeyCode::D) {
            dir += Vec3::X;
        }

        if dir != Vec3::ZERO {
            //tf.translation += dir * time.delta_seconds() * 400.0;
            dir *= 400.0;
            message.send(ClientMessage::MoveMe(dir.xy()));
        }
    }
}

fn startup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn start_client_networking(mut commands: Commands) {
    let server = RenetClient::new(ConnectionConfig::default());

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let client_id = HOST.load(Ordering::Relaxed) as u64; //current_time.as_millis() as u64;
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
    commands.insert_resource(server);
    //commands.spawn((
    //    Player,
    //    SpriteBundle {
    //        sprite: Sprite {
    //            color: if HOST.load(Ordering::Relaxed) {
    //                Color::GREEN
    //            } else {
    //                Color::BLUE
    //            },
    //            custom_size: Some(Vec2::splat(100.0)),
    //            ..default()
    //        },
    //        ..default()
    //    },
    //));
}
