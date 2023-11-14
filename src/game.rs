use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use bevy_renet::renet::ServerEvent;
use leafwing_input_manager::prelude::ActionState;
use serde::{Deserialize, Serialize};

use crate::player::{Action, Player, PlayerPlugin};
use crate::prediction::PredictionPlugin;
use crate::replicate::schedule::{NetworkBlueprint, NetworkUpdate};
use crate::replicate::{is_server, AppExt, Replicate, ReplicationPlugin, Owner};

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
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, spawn_avatar.run_if(is_server))
        .add_systems(NetworkBlueprint, block_blueprint)
        .add_systems(NetworkUpdate, spawn_block);
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
