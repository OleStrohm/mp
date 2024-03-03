use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::window::PrimaryWindow;
use leafwing_input_manager::axislike::DualAxisData;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::prediction::{resimulating, CommitActions};
use crate::replicate::schedule::{NetworkBlueprint, NetworkPreUpdate, NetworkUpdate};
use crate::replicate::{AppExt, Owner};

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub color: Color,
    pub controller: Owner,
}

#[derive(Actionlike, Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, TypePath)]
pub enum Action {
    Main,
    Up,
    Down,
    Left,
    Right,
    Shoot,
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<Action>::default())
            .replicate::<Player>()
            .add_systems(
                NetworkBlueprint,
                (player_blueprint, make_player_controllable).chain(),
            )
            .add_systems(
                NetworkPreUpdate,
                update_mouse_pos
                    .run_if(not(resimulating))
                    .before(CommitActions),
            )
            .add_systems(NetworkUpdate, rotate_player)
        ;
    }
}

fn rotate_player(mut controlled_players: Query<(&mut Transform, &ActionState<Action>)>) {
    for (mut tf, actions) in &mut controlled_players {
        if let Some(pos) = actions.axis_pair(Action::Main) {
            let m_pos = pos.xy();
            let point_dir = m_pos - tf.translation.xy();

            tf.rotation =
                Quat::from_rotation_arc(Vec3::X, point_dir.normalize_or_zero().extend(0.0));
        }
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
            actions.action_data_mut(Action::Shoot).axis_pair = Some(DualAxisData::from_xy(m_pos));
        }
    };
}

fn player_blueprint(
    mut commands: Commands,
    new_players: Query<(Entity, &Transform, &Player), Added<Player>>,
    client_id: Option<Res<Owner>>,
) {
    for (entity, &transform, player) in &new_players {
        let color = player.color;
        let in_control = client_id
            .as_ref()
            .map(|id| player.controller == **id)
            .unwrap_or(false);

        commands.entity(entity).insert((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::splat(1.0)),
                    ..default()
                },
                transform,
                ..default()
            },
            player.controller,
            Name::from(format!("Player - {}", player.name)),
        ));

        if in_control {
            commands.entity(entity).insert(Control);
        }
    }
}

fn make_player_controllable(
    mut commands: Commands,
    mut controlled_players: Query<(Entity, &mut Transform), (With<Player>, Added<Control>)>,
) {
    for (entity, mut tf) in &mut controlled_players {
        tf.translation.z = 1.0;
        let mut input_map = InputMap::default();
        input_map
            .insert_multiple([
                (KeyCode::W, Action::Up),
                (KeyCode::A, Action::Left),
                (KeyCode::S, Action::Down),
                (KeyCode::D, Action::Right),
                (KeyCode::Space, Action::Shoot),
            ])
            .insert_multiple([(MouseButton::Left, Action::Shoot)]);

        commands
            .entity(entity)
            .insert(InputManagerBundle::<Action> {
                action_state: default(),
                input_map,
            })
            .with_children(|entity| {
                entity.spawn(SpriteBundle {
                    sprite: Sprite {
                        color: Color::GOLD,
                        custom_size: Some(Vec2::splat(0.3)),
                        ..default()
                    },
                    transform: Transform::from_xyz(0.0, 0.0, 1.0),
                    ..default()
                });
            });
    }
}
