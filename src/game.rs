use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use bevy_renet::renet::ServerEvent;
use leafwing_input_manager::prelude::ActionState;
use serde::{Deserialize, Serialize};

use crate::player::{Action, Player, PlayerPlugin};
use crate::prediction::PredictionPlugin;
use crate::replicate::schedule::{NetworkBlueprint, NetworkFixedTime, NetworkUpdate};
use crate::replicate::{is_server, AppExt, Owner, Replicate, ReplicationPlugin};

pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ReplicationPlugin::with_step(FIXED_TIMESTEP),
            PredictionPlugin::<Action>::default(),
            PlayerPlugin,
        ))
        .replicate::<Block>()
        .replicate::<Npc>()
        .replicate::<Dir>()
        .add_systems(Startup, spawn_camera)
        .add_systems(NetworkBlueprint, (block_blueprint, npc_blueprint))
        .add_systems(
            NetworkUpdate,
            (
                spawn_block,
                spawn_avatar.run_if(is_server),
                spawn_npc.run_if(is_server.and_then(run_once())),
                npc_move,
            ),
        );
    }
}

#[derive(Component, Serialize, Deserialize)]
struct Block {
    pos: Vec3,
}

fn block_blueprint(mut commands: Commands, new_blocks: Query<(Entity, &Block), Added<Block>>) {
    for (entity, block) in &new_blocks {
        commands.entity(entity).insert(SpriteBundle {
            sprite: Sprite {
                color: Color::GRAY,
                ..default()
            },
            transform: Transform::from_translation(block.pos),
            ..default()
        });
    }
}

fn spawn_block(mut commands: Commands, players: Query<&ActionState<Action>>) {
    for actions in &players {
        if actions.just_pressed(Action::Main) {
            if let Some(pos) = actions.axis_pair(Action::Main) {
                commands.spawn((
                    Replicate,
                    Block {
                        pos: pos.xy().extend(0.0),
                    },
                ));
            }
        }
    }
}

fn spawn_camera(mut commands: Commands) {
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

fn spawn_avatar(mut commands: Commands, mut events: EventReader<ServerEvent>) {
    for event in events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                let color = Color::rgb(rand::random(), rand::random(), rand::random());
                let pos = 4.0 * Vec2::new(rand::random(), rand::random());

                let avatar = commands
                    .spawn((
                        Replicate,
                        Player {
                            color,
                            controller: Owner(client_id.raw()),
                        },
                        Transform::from_translation(pos.extend(0.0)),
                    ))
                    .id();

                println!("{client_id} connected! Creating the avatar as {avatar:?}");
            }
            ServerEvent::ClientDisconnected {
                client_id: _client_id,
                reason: _reason,
            } => {
                println!("{_client_id} disconnected ({_reason})");
            }
        }
    }
}

#[derive(Debug, Component, Serialize, Deserialize)]
struct Npc {
    color: Color,
    speed: f32,
}

#[derive(Debug, Component, Serialize, Deserialize)]
enum Dir {
    Left,
    Right,
}

fn npc_move(mut npcs: Query<(&mut Transform, &mut Dir, &Npc)>, time: Res<NetworkFixedTime>) {
    for (mut tf, mut dir, Npc { speed, .. }) in &mut npcs {
        match *dir {
            Dir::Left => {
                tf.translation.x -= speed * time.duration().as_secs_f32();
                if tf.translation.x <= -5.0 {
                    *dir = Dir::Right;
                }
            }
            Dir::Right => {
                tf.translation.x += speed * time.duration().as_secs_f32();
                if tf.translation.x >= 5.0 {
                    *dir = Dir::Left;
                }
            }
        }
    }
}

fn npc_blueprint(mut commands: Commands, npcs: Query<(Entity, &Transform, &Npc), Without<Sprite>>) {
    for (entity, &transform, &Npc { color, .. }) in &npcs {
        commands.entity(entity).insert(SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some((1.0, 1.0).into()),
                ..default()
            },
            transform,
            ..default()
        });
    }
}

fn spawn_npc(mut commands: Commands) {
    commands.spawn((
        Replicate,
        Npc {
            color: Color::PURPLE,
            speed: 5.0,
        },
        Dir::Left,
        Transform::from_xyz(5.0, 0.0, -2.0),
    ));
}
