use crate::player::{ActionHistory, Action, Control};
use crate::replicate::schedule::NetworkPreUpdate;
use crate::replicate::{Channel, NetworkEntities, NetworkTick, SyncedServerTick};
use bevy::prelude::*;
use bevy_renet::renet::{RenetClient, RenetServer};
use leafwing_input_manager::prelude::ActionState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Resource, Default)]
pub struct Resimulating;

pub struct PredictionPlugin;

#[derive(Debug, SystemSet, Clone, PartialEq, Eq, Hash)]
pub struct SendClientInput;
#[derive(Debug, SystemSet, Clone, PartialEq, Eq, Hash)]
pub struct ReceiveClientInput;

impl Plugin for PredictionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            NetworkPreUpdate,
            (
                (
                    copy_input_for_tick
                        .run_if(not(resimulating))
                        .before(SendClientInput),
                    send_client_input
                        .run_if(
                            crate::transport::client_connected()
                                .or_else(bevy_renet::transport::client_connected()),
                        )
                        .run_if(not(resimulating))
                        .in_set(SendClientInput),
                )
                    .chain(),
                copy_input_from_history.run_if(resimulating),
                (
                    receive_client_input,
                    apply_deferred,
                    copy_input_from_history,
                    apply_deferred,
                )
                    .chain()
                    .run_if(resource_exists::<RenetServer>()),
            ),
        );
    }
}

fn copy_input_for_tick(
    mut action_query: Query<(&ActionState<Action>, &mut ActionHistory), With<Control>>,
    tick: Res<NetworkTick>,
    last_server_tick: Option<Res<SyncedServerTick>>,
) {
    for (actions, mut history) in &mut action_query {
        history.add_for_tick(*tick, actions.clone());

        let Some(last_server_tick) = last_server_tick.as_deref() else { continue };
        history.remove_old_history(last_server_tick.tick);
    }
}

#[derive(Serialize, Deserialize)]
pub struct InputPacket {
    pub entity: Entity,
    pub tick: NetworkTick,
    pub history: ActionHistory,
}

fn send_client_input(
    mut client: ResMut<RenetClient>,
    history: Query<(Entity, &ActionHistory)>,
    tick: Res<NetworkTick>,
    network_entities: Res<NetworkEntities>,
) {
    let Ok((entity, history)) = history.get_single() else { return };

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

fn receive_client_input(mut commands: Commands, mut server: ResMut<RenetServer>) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, Channel::ReliableOrdered) {
            let packet = bincode::deserialize::<InputPacket>(&message).unwrap();
            let history = packet.history;

            commands.entity(packet.entity).insert(history);
        }
    }
}

pub fn copy_input_from_history(
    mut commands: Commands,
    mut players: Query<(Entity, &mut ActionHistory)>,
    tick: Res<NetworkTick>,
) {
    for (player, history) in &mut players {
        let mut player = commands.entity(player);

        let Some(actions) = history.at_tick(*tick) else { continue };
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
