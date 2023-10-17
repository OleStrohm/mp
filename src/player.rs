use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::window::PrimaryWindow;
use bevy_renet::RenetClientPlugin;
use itertools::multizip;
use leafwing_input_manager::axislike::DualAxisData;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::prediction::{resimulating, CommitActions};
use crate::replicate::schedule::{
    NetworkBlueprint, NetworkFixedTime, NetworkPreUpdate, NetworkUpdate,
};
use crate::replicate::{is_client, AppExt, ClientId};

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player {
    pub color: Color,
    pub controller: ClientId,
}

#[derive(Actionlike, Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, TypePath)]
pub enum Action {
    Main,
    Up,
    Down,
    Left,
    Right,
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<RenetClientPlugin>() {
            app.add_plugins(InputManagerPlugin::<Action>::default());
        }

        app.replicate::<Player>()
            .replicate::<Control>()
            .replicate_with::<Transform>(
                |component| bincode::serialize(&component.translation).unwrap(),
                |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
            )
            .add_systems(
                NetworkBlueprint,
                (
                    player_blueprint.run_if(resource_exists::<ClientId>()),
                    apply_deferred,
                )
                    .chain(),
            )
            .add_systems(
                NetworkPreUpdate,
                update_mouse_pos
                    .run_if(is_client)
                    .run_if(not(resimulating))
                    .before(CommitActions),
            )
            .add_systems(NetworkUpdate, (handle_input).chain());
    }
}

fn update_mouse_pos(
    mut action_query: Query<&mut ActionState<Action>, With<Control>>,
    camera: Query<(&Camera, &GlobalTransform)>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let (camera, camera_tf) = camera.single();
    let window = window.single();

    if let Some(m_pos) = window
        .cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_tf, p))
    {
        for mut actions in &mut action_query {
            actions.action_data_mut(Action::Main).axis_pair = Some(DualAxisData::from_xy(m_pos));
        }
    };
}

fn handle_input(
    mut players: ParamSet<(
        Query<(Entity, &mut Transform, &ActionState<Action>), With<Player>>,
        Query<(Entity, &Transform), With<Player>>,
    )>,
    fixed_time: Res<NetworkFixedTime>,
) {
    let new_position = players
        .p0()
        .iter()
        .map(|(e, tf, actions)| {
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

            let movement = 6.0 * dir * fixed_time.period.as_secs_f32();

            (e, tf.translation + movement.extend(0.0))
        })
        .collect::<Vec<_>>();

    let can_move = new_position
        .iter()
        .map(|(e, p)| {
            players
                .p1()
                .iter()
                .filter(|(other_entity, _)| e != other_entity)
                .map(|(_, other_tf)| {
                    let diff = p.xy() - other_tf.translation.xy();
                    diff.to_array()
                        .into_iter()
                        .any(|distance| distance.abs() >= 1.0)
                })
                .all(|outside| outside)
        })
        .collect::<Vec<_>>();

    for (mut tf, new_pos, can_move) in multizip((
        players.p0().iter_mut().map(|(_, tf, _)| tf),
        new_position.into_iter().map(|(_, p)| p),
        can_move,
    )) {
        if can_move {
            tf.translation = new_pos;
        }
    }
}

fn player_blueprint(
    mut commands: Commands,
    new_players: Query<(Entity, &Player), Added<Player>>,
    client_id: Res<ClientId>,
) {
    for (entity, player) in &new_players {
        let color = player.color;
        let in_control = player.controller == *client_id;

        commands.entity(entity).insert(SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
            ..default()
        });

        if in_control {
            let mut input_map = InputMap::default();
            input_map
                .insert_multiple([
                    (KeyCode::W, Action::Up),
                    (KeyCode::A, Action::Left),
                    (KeyCode::S, Action::Down),
                    (KeyCode::D, Action::Right),
                ])
                .insert_multiple([(MouseButton::Left, Action::Main)]);

            commands
                .entity(entity)
                .insert((
                    Control,
                    InputManagerBundle::<Action> {
                        action_state: default(),
                        input_map,
                    },
                ))
                .with_children(|entity| {
                    entity.spawn(SpriteBundle {
                        sprite: Sprite {
                            color: Color::GOLD,
                            custom_size: Some(Vec2::splat(1.05)),
                            ..default()
                        },
                        ..default()
                    });
                });
        }
    }
}
