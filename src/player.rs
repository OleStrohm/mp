use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::window::PrimaryWindow;
use bevy_renet::RenetClientPlugin;
use leafwing_input_manager::axislike::DualAxisData;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::prediction::{resimulating, CommitActions};
use crate::replicate::schedule::{NetworkBlueprint, NetworkPreUpdate};
use crate::replicate::{is_client, AppExt, Owner};

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player {
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
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<Action>::default())
            .replicate::<Player>()
            .replicate::<Control>()
            .replicate_with::<Transform>(
                |component| bincode::serialize(&component.translation).unwrap(),
                |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
            )
            .add_systems(NetworkBlueprint, player_blueprint)
            .add_systems(
                NetworkPreUpdate,
                update_mouse_pos
                    .run_if(is_client)
                    .run_if(not(resimulating))
                    .before(CommitActions),
            );
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
            Name::from(format!("Player - {}", player.controller.0)),
        ));

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
                            custom_size: Some(Vec2::splat(0.3)),
                            ..default()
                        },
                        transform: Transform::from_xyz(0.0, 0.0, 1.0),
                        ..default()
                    });
                });
        }
    }
}
