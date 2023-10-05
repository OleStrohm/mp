use std::collections::VecDeque;

use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy_renet::RenetClientPlugin;
use itertools::multizip;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::replicate::schedule::{NetworkBlueprint, NetworkFixedTime, NetworkUpdate};
use crate::replicate::{AppExt, ClientId, NetworkTick, Predict};

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct Control;

#[derive(Component, Serialize, Deserialize)]
pub struct Player {
    pub color: Color,
    pub controller: ClientId,
}

#[derive(Actionlike, Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, TypePath)]
pub enum Action {
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
            .add_systems(NetworkUpdate, (handle_input).chain());
    }
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
            commands
                .entity(entity)
                .insert((
                    Predict,
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
                    ActionHistory::default(),
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

#[derive(Debug, Component, Serialize, Deserialize, Clone, Default)]
pub struct ActionHistory {
    pub tick: NetworkTick,
    pub history: VecDeque<ActionState<Action>>,
}

impl ActionHistory {
    pub fn add_for_tick(&mut self, tick: NetworkTick, actions: ActionState<Action>) {
        self.tick = tick;
        self.history.push_front(actions);
    }

    pub fn at_tick(&self, at: NetworkTick) -> Option<ActionState<Action>> {
        if self.tick < at {
            return None;
        }

        self.history.get((self.tick.0 - at.0) as usize).cloned()
    }

    pub fn remove_old_history(&mut self, oldest: NetworkTick) {
        let history_len = 1 + self.tick.0.saturating_sub(oldest.0);

        while self.history.len() > history_len as usize {
            self.history.pop_back();
        }
    }
}
