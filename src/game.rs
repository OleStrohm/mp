use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use bevy_renet::{RenetServerPlugin, RenetClientPlugin};
use bevy_renet::renet::ServerEvent;
use bevy_renet::transport::{NetcodeServerPlugin, NetcodeClientPlugin};

use crate::player::{Player, PlayerPlugin};
use crate::prediction::PredictionPlugin;
use crate::replicate::{is_server, ClientId, Replicate, ReplicationPlugin};

pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RenetServerPlugin,
            NetcodeServerPlugin,
            RenetClientPlugin,
            NetcodeClientPlugin,
            ReplicationPlugin::with_step(FIXED_TIMESTEP),
            PredictionPlugin,
            PlayerPlugin,
        ))
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, spawn_avatar.run_if(is_server));
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
