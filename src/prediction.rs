use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::player::Control;
use crate::replicate::schedule::NetworkPreUpdate;
use crate::replicate::{Channel, NetworkEntities, NetworkTick, SyncedServerTick};
use crate::transport;
use bevy::prelude::*;
use bevy_renet::client_connected;
use bevy_renet::renet::{RenetClient, RenetServer};
use leafwing_input_manager::buttonlike::ButtonState;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

#[derive(Debug, Resource, Default)]
pub struct Resimulating;

pub struct PredictionPlugin<A>(PhantomData<A>);

#[derive(Debug, SystemSet, Clone, PartialEq, Eq, Hash)]
pub struct CommitActions;

impl<A: Actionlike + Serialize + for<'a> Deserialize<'a> + Send + Sync + 'static> Plugin
    for PredictionPlugin<A>
{
    fn build(&self, app: &mut App) {
        app.add_systems(
            NetworkPreUpdate,
            (
                (
                    copy_input_for_tick::<A>,
                    apply_deferred,
                    send_client_input::<A>
                        .run_if(client_connected().or_else(transport::client_connected())),
                )
                    .chain()
                    .run_if(not(resimulating))
                    .in_set(CommitActions),
                copy_input_from_history::<A>.run_if(resimulating),
                (
                    receive_client_input::<A>,
                    apply_deferred,
                    copy_input_from_history::<A>,
                    apply_deferred,
                )
                    .chain()
                    .run_if(resource_exists::<RenetServer>()),
            ),
        );
    }
}

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct ActionHistory<A: Actionlike> {
    pub tick: NetworkTick,
    pub history: VecDeque<ActionState<A>>,
}

impl<A: Actionlike> Default for ActionHistory<A> {
    fn default() -> Self {
        ActionHistory {
            tick: NetworkTick::default(),
            history: VecDeque::default(),
        }
    }
}

impl<A: Actionlike> ActionHistory<A> {
    pub fn add_for_tick(&mut self, tick: NetworkTick, actions: ActionState<A>) {
        self.tick = tick;
        self.history.push_front(actions);
    }

    pub fn at_tick(&self, at: NetworkTick) -> Option<ActionState<A>> {
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

fn copy_input_for_tick<A: Actionlike + Send + Sync + 'static>(
    mut commands: Commands,
    mut action_query: Query<
        (Entity, &ActionState<A>, Option<&mut ActionHistory<A>>),
        With<Control>,
    >,
    tick: Res<NetworkTick>,
    last_server_tick: Option<Res<SyncedServerTick>>,
) {
    for (entity, actions, history) in &mut action_query {
        match history {
            Some(mut history) => {
                let mut actions = actions.clone();
                let prev_actions = history.history.front().unwrap().clone();

                for a in actions.get_pressed() {
                    if !prev_actions.pressed(a.clone()) {
                        actions.action_data_mut(a).state = ButtonState::JustPressed;
                    }
                }
                for a in actions.get_released() {
                    if !prev_actions.released(a.clone()) {
                        actions.action_data_mut(a.clone()).state = ButtonState::JustReleased;
                    }
                }
                history.add_for_tick(*tick, actions);

                let Some(last_server_tick) = last_server_tick.as_deref() else {
                    continue;
                };

                history.remove_old_history(last_server_tick.tick);
            }
            None => {
                let mut history = ActionHistory::<A>::default();
                history.add_for_tick(*tick, actions.clone());

                commands.entity(entity).insert(history);
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct InputPacket<A: Actionlike> {
    pub entity: Entity,
    pub tick: NetworkTick,
    pub history: ActionHistory<A>,
}

fn send_client_input<A: Actionlike + Send + Sync + Serialize + 'static>(
    mut client: ResMut<RenetClient>,
    history: Query<(Entity, &ActionHistory<A>)>,
    tick: Res<NetworkTick>,
    network_entities: Res<NetworkEntities>,
) {
    let Ok((entity, history)) = history.get_single() else {
        println!("Could not find entity");
        return;
    };
    println!("Sending history for {entity:?}");

    let server_entity = *network_entities
        .iter()
        .find(|&(_, &client_entity)| client_entity == entity)
        .unwrap()
        .0;

    let packet = InputPacket {
        entity: server_entity,
        tick: *tick,
        history: history.clone(),
    };

    client.send_message(
        Channel::ReliableOrdered,
        bincode::serialize(&packet).unwrap(),
    );
}

fn receive_client_input<A: Actionlike + for<'a> Deserialize<'a> + Send + Sync + 'static>(
    mut commands: Commands,
    mut server: ResMut<RenetServer>,
) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, Channel::ReliableOrdered) {
            let packet = bincode::deserialize::<InputPacket<A>>(&message).unwrap();
            commands.entity(packet.entity).insert(packet.history);
        }
    }
}

pub fn copy_input_from_history<A: Actionlike + Send + Sync + 'static>(
    mut commands: Commands,
    mut players: Query<(Entity, &mut ActionHistory<A>)>,
    tick: Res<NetworkTick>,
) {
    for (player, history) in &mut players {
        let mut player = commands.entity(player);

        let Some(actions) = history.at_tick(*tick) else {
            continue;
        };
        player.insert(actions);
    }
}

pub fn is_desynced(_world: &mut World) -> bool {
    //let new_replicated_entities = world
    //    .query_filtered::<(), Added<Replicate>>()
    //    .iter(world)
    //    .count();

    //if new_replicated_entities > 0 {
    //    return true;
    //}

    //for (tf, predicted_tf) in world.query_filtered::<(&Transform, &Replicated<Transform>), With<Predict>>().iter(world) {
    //    tf.translation.abs_diff_eq(predicted_tf.translation, 0.01);
    //}

    true
}

pub fn resimulating(resimulating: Option<Res<Resimulating>>) -> bool {
    resimulating.is_some()
}

impl<A> Default for PredictionPlugin<A> {
    fn default() -> Self {
        PredictionPlugin(PhantomData)
    }
}
