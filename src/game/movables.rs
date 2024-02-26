use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::player::{Action, Player};
use crate::replicate::schedule::{NetworkFixedTime, NetworkUpdate};
use crate::replicate::AppExt;

use super::{Npc, NpcAction};

pub struct MovablePlugin;

impl Plugin for MovablePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<NpcAction>::default());

        app.replicate_with::<Transform>(
            |component| {
                bincode::serialize(&(component.translation, component.rotation, component.scale))
                    .unwrap()
            },
            |data| {
                let (translation, rotation, scale) = bincode::deserialize(data).unwrap();
                Transform {
                    translation,
                    rotation,
                    scale,
                }
            },
        )
        .add_systems(NetworkUpdate, handle_movement);
    }
}

fn handle_movement(
    mut moving_entities: ParamSet<(
        (
            Query<(Entity, &Transform, &ActionState<Action>), With<Player>>,
            Query<
                (Entity, &Transform, &ActionState<NpcAction>, &Npc),
                (With<Npc>, Without<Player>),
            >,
        ),
        Query<(Entity, &Transform), Or<(With<Player>, With<Npc>)>>,
        Query<(Entity, &mut Transform)>,
    )>,
    fixed_time: Res<NetworkFixedTime>,
) {
    let (players, npcs) = moving_entities.p0();
    let new_position = players
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

            let movement = 6.0 * dir * fixed_time.duration().as_secs_f32();

            (e, tf.translation + movement.extend(0.0))
        })
        .chain(npcs.iter().map(|(e, tf, actions, npc)| {
            let mut dir = Vec2::splat(0.0);
            if actions.pressed(NpcAction::Up) {
                dir.y += 1.0;
            }
            if actions.pressed(NpcAction::Down) {
                dir.y -= 1.0;
            }
            if actions.pressed(NpcAction::Left) {
                dir.x -= 1.0;
            }
            if actions.pressed(NpcAction::Right) {
                dir.x += 1.0;
            }
            let movement = npc.speed * dir * fixed_time.duration().as_secs_f32();
            (e, tf.translation + movement.extend(0.0))
        }))
        .collect::<Vec<_>>();

    for (entity, new_pos) in new_position {
        let mut query = moving_entities.p2();
        let (_, mut tf) = query.get_mut(entity).unwrap();
        tf.translation = new_pos;
    }
}
