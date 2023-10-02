use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use bevy::audio::AudioPlugin;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::camera::ScalingMode;
use bevy_renet::renet::transport::{ClientAuthentication, NetcodeClientTransport};
use bevy_renet::renet::RenetClient;
use bevy_renet::transport::{client_connected, NetcodeClientPlugin};
use bevy_renet::RenetClientPlugin;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::replicate::schedule::*;
use crate::replicate::{
    AppExt, Channel, NetworkTick, ReplicationConnectionConfig, ReplicationPlugin, PROTOCOL_ID,
};
use crate::shared::{SharedPlugin, FIXED_TIMESTEP};

static HOST: AtomicBool = AtomicBool::new(false);

#[derive(Resource, Deref, DerefMut, Serialize, Deserialize, PartialEq)]
pub struct ClientId(pub u64);

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct Control;

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
            RenetClientPlugin,
            NetcodeClientPlugin,
            SharedPlugin,
            ReplicationPlugin::with_step(FIXED_TIMESTEP),
            InputManagerPlugin::<Action>::default(),
        ))
        .replicate::<Control>()
        .replicate::<Player>()
        .replicate_with::<Transform>(
            |component| bincode::serialize(&component.translation).unwrap(),
            |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
        )
        .add_systems(Startup, (startup, start_client_networking))
        .add_systems(
            Update,
            (
                player_blueprint,
                send_client_messages.run_if(client_connected()),
            ),
        )
        .add_systems(NetworkPreUpdate, copy_input_for_tick)
        .add_systems(NetworkUpdate, handle_input)
        .add_systems(NetworkBlueprint, player_blueprint)
        .run();
}

fn copy_input_for_tick(tick: Res<NetworkTick>, inputs: Query<&ActionState<Action>, With<Control>>) {

}

#[derive(Actionlike, Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, TypePath)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Serialize, Deserialize)]
pub struct ClientPacket {
    pub time: Duration,
    pub tick: NetworkTick,
    pub actions: ActionState<Action>,
}

fn send_client_messages(
    mut client: ResMut<RenetClient>,
    player: Query<&ActionState<Action>, (With<Player>, With<Control>)>,
) {
    let Ok(actions) = player.get_single() else { return };

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let packet = ClientPacket {
        time,
        tick: NetworkTick(0),
        actions: actions.clone(),
    };

    client.send_message(
        Channel::ReliableOrdered,
        bincode::serialize(&packet).unwrap(),
    );
}

#[derive(Component, Serialize, Deserialize)]
pub struct Player {
    pub color: Color,
    pub controller: ClientId,
}

fn player_blueprint(
    mut commands: Commands,
    new_players: Query<(Entity, &Player), Added<Player>>,
    client_id: Res<ClientId>,
) {
    for (other, player) in &new_players {
        let color = player.color;
        let in_control = player.controller == *client_id;
        commands.entity(other).insert(SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
            ..default()
        });

        if in_control {
            commands.entity(other).insert((
                Control,
                InputManagerBundle::<Action> {
                    action_state: default(),
                    input_map: InputMap::new([
                        (KeyCode::W, Action::Up),
                        (KeyCode::A, Action::Left),
                        (KeyCode::S, Action::Down),
                        (KeyCode::D, Action::Right),
                    ]),
                },
            ));
        }
    }
}

fn handle_input(mut players: Query<(&mut Transform, &ActionState<Action>), With<Player>>) {
    for (mut tf, actions) in &mut players {
        let mut dir = Vec2::splat(0.0);
        if actions.pressed(Action::Up) {
            dir.y += 1.0;
        }
        if actions.pressed(Action::Down) {
            dir.y -= 1.0;
        }
        if actions.pressed(Action::Left) {
            dir.x -= 1.0;
        }
        if actions.pressed(Action::Right) {
            dir.x += 1.0;
        }

        tf.translation += 6.0 * dir.extend(0.0) * FIXED_TIMESTEP;
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
    commands.insert_resource(ClientId(client_id));
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
