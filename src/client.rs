use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::{ClientAuthentication, NetcodeClientTransport};
use bevy_renet::renet::RenetClient;
use bevy_renet::transport::{client_connected, NetcodeClientPlugin};
use bevy_renet::RenetClientPlugin;
use serde::{Deserialize, Serialize};

use crate::replicate::{Channel, ReplicationConnectionConfig, NetworkTick, PROTOCOL_ID};
use crate::server::{PlayerData, ServerMessage, ServerPacket};
use crate::shared::{SharedPlugin, FIXED_TIMESTEP};

static HOST: AtomicBool = AtomicBool::new(false);

#[derive(Resource, Deref, DerefMut)]
pub struct ClientId(u64);

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

    client.send_message(
        Channel::ReliableOrdered,
        bincode::serialize(&packet).unwrap(),
    );
}

fn receive_server_messages(
    mut commands: Commands,
    mut network_entities: ResMut<NetworkEntities>,
    mut client: ResMut<RenetClient>,
    mut tick: Option<ResMut<NetworkTick>>,
    mut players: Query<&mut Transform>,
    this_client_id: Res<ClientId>,
) {
    let Some(packet) = client.receive_message(Channel::ReliableOrdered) else { return };
    let packet: ServerPacket = bincode::deserialize(&packet).unwrap();

    if !packet.messages.is_empty() {
        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        println!(
            "Received {} message(s) at Server tick: {:?} (Client {:?})    |     {:?}",
            packet.messages.len(),
            packet.tick.0,
            tick,
            time - packet.time,
        );
    }

    if let Some(ref mut tick) = tick {
        **tick = packet.tick;
    }

    let mut tick = tick.as_deref_mut().copied();

    for message in packet.messages {
        match message {
            ServerMessage::FullSync(client_id, state) => {
                if client_id == this_client_id.0 {
                    println!("Syncing! {:?}", state.players);
                    for PlayerData {
                        network_id,
                        pos,
                        color,
                    } in state.players
                    {
                        let spawned = commands
                            .spawn((
                                Transform::from_translation(pos.extend(0.0)),
                                PlayerColor(color),
                                Player,
                            ))
                            .id();

                        network_entities.insert(network_id, spawned);
                    }

                    tick = Some(packet.tick);
                    commands.insert_resource(packet.tick);
                }
            }
            message if tick.is_none() => {
                println!("Skipping {:?} because this client is not synced", message)
            }
            ServerMessage::AssignControl(client_id, network_id) => {
                if client_id == this_client_id.0 {
                    let local_entity = *network_entities.get(&network_id).unwrap();
                    commands.entity(local_entity).insert(Control);
                }
            }
            ServerMessage::SpawnEntity(PlayerData {
                network_id,
                pos,
                color,
            }) => {
                let spawned = commands
                    .spawn((
                        Player,
                        Transform::from_translation(pos.extend(0.0)),
                        PlayerColor(color),
                    ))
                    .id();
                network_entities.insert(network_id, spawned);
            }
            ServerMessage::MoveEntity(network_id, dir) => {
                let local_entity = *network_entities.get(&network_id).unwrap();
                players.get_mut(local_entity).unwrap().translation +=
                    dir.extend(0.0) * FIXED_TIMESTEP;
            }
        }
    }
}

#[derive(Component, Serialize, Deserialize)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player;

#[derive(Component)]
pub struct PlayerColor(pub Color);

fn add_visual_for_other_players(
    mut commands: Commands,
    others: Query<(Entity, Option<&Transform>, &PlayerColor), (Added<Player>, Without<Sprite>)>,
) {
    for (other, tf, color) in &others {
        commands.entity(other).insert(SpriteBundle {
            sprite: Sprite {
                color: color.0,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
            transform: tf.copied().unwrap_or_default(),
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
            dir *= 4.0;
            message.send(ClientMessage::MoveMe(dir.xy()));
        }
    }
}

fn startup(mut commands: Commands) {
    commands.spawn(Camera2dBundle {
        projection: OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical(10.0),
            far: 1000.0,
            near: -1000.0,
            ..Default::default()
        },
        ..Default::default()
    });
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
    commands.insert_resource(ClientId(client_id))
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
