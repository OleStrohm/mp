use std::time::Duration;

use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::transport::NetcodeTransportError;
use bevy_renet::renet::{ChannelConfig, ConnectionConfig, RenetClient, RenetServer, SendType};
use bevy_renet::transport::{NetcodeClientPlugin, NetcodeServerPlugin};
use bevy_renet::{RenetClientPlugin, RenetReceive, RenetSend, RenetServerPlugin};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use self::schedule::{
    run_network_fixed, NetworkFixedTime, NetworkResync, NetworkScheduleOrder, NetworkUpdateTick,
    TickStrategy,
};

#[cfg(test)]
mod tests;

pub mod schedule;

pub const PROTOCOL_ID: u64 = 7;

#[derive(Debug, Resource, Serialize, Deserialize, PartialEq, Component, Clone, Copy)]
pub enum Owner {
    Server,
    Client(u64),
}

#[derive(Resource, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, PartialOrd)]
pub struct NetworkTick(pub u64);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct NetworkEntities(HashMap<Entity, Entity>);

#[derive(Component, Clone, Copy)]
pub struct Replicate;

#[repr(u8)]
pub enum Channel {
    Replication = 0,
    ClientInput,
    ReliableOrdered,
}

impl From<Channel> for u8 {
    fn from(channel: Channel) -> Self {
        channel as u8
    }
}

#[derive(Resource, Debug, Default, Clone)]
pub struct SyncedServerTick {
    //sent_at: Duration,
    //received_at: Duration,
    pub tick: NetworkTick,
}

#[derive(Debug, Component, Deref, DerefMut)]
pub struct Replicated<T>(pub T);

pub struct ReplicationPlugin {
    period: f32,
    tick_strategy: TickStrategy,
}

impl ReplicationPlugin {
    pub fn new(period: f32, tick_strategy: TickStrategy) -> Self {
        ReplicationPlugin {
            period,
            tick_strategy,
        }
    }

    pub fn with_step(period: f32) -> Self {
        ReplicationPlugin::new(period, TickStrategy::Automatic)
    }
}

impl Plugin for ReplicationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RenetServerPlugin,
            RenetClientPlugin,
            NetcodeServerPlugin,
            NetcodeClientPlugin,
        ))
        .init_resource::<ReplicationFunctions>()
        .init_resource::<NetworkScheduleOrder>()
        .init_resource::<NetworkTick>()
        .init_resource::<NetworkEntities>()
        .insert_resource(NetworkFixedTime(Timer::from_seconds(
            self.period,
            TimerMode::Repeating,
        )))
        .insert_resource(self.tick_strategy)
        .add_systems(Update, panic_on_error_system)
        .add_systems(
            PreUpdate,
            receive_updated_components
                .after(RenetReceive)
                .run_if(is_client),
        )
        .add_systems(Update, run_network_fixed)
        .add_systems(
            PostUpdate,
            send_updated_components.before(RenetSend).run_if(is_server),
        )
        .add_systems(NetworkUpdateTick, increment_tick)
        .add_systems(
            NetworkResync,
            (apply_deferred.after(CopyReplicated), reset_to_server_tick),
        );
    }
}

fn increment_tick(mut tick: ResMut<NetworkTick>) {
    tick.0 += 1;
}

fn reset_to_server_tick(
    mut commands: Commands,
    predicted_spawns: Query<Entity, With<Replicate>>,
    mut tick: ResMut<NetworkTick>,
    synced_server_tick: Res<SyncedServerTick>,
) {
    for entity in &predicted_spawns {
        commands.entity(entity).despawn_recursive();
    }
    *tick = synced_server_tick.tick;
}

#[derive(Debug, SystemSet, Clone, PartialEq, Eq, Hash)]
struct CopyReplicated;

fn copy_replicated_component<T: Component>(world: &mut World) {
    for entity in world
        .query_filtered::<Entity, With<Replicated<T>>>()
        .iter(world)
        .collect::<Vec<_>>()
    {
        let mut entity = world.entity_mut(entity);
        let component = entity.take::<Replicated<T>>().unwrap().0;
        entity.insert(component);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ReplicationPacket {
    tick: NetworkTick,
    updates: Vec<EntityUpdates>,
    despawns: Vec<Entity>,
}

fn send_updated_components(world: &mut World) {
    let updates = world
        .query_filtered::<Entity, With<Replicate>>()
        .iter(world)
        .map(|entity| serialize_all_components(world, entity))
        .collect();

    let despawns = world
        .removed_components()
        .get(world.component_id::<Replicate>().unwrap())
        .map(|events| {
            events
                .get_reader()
                .read(events)
                .map(|e| e.clone().into())
                .collect_vec()
        })
        .unwrap_or_default();

    let tick = *world.resource::<NetworkTick>();

    let packet = ReplicationPacket {
        tick,
        updates,
        despawns,
    };
    let mut server = world.resource_mut::<RenetServer>();

    server.broadcast_message(Channel::Replication, bincode::serialize(&packet).unwrap());
}

fn receive_updated_components(world: &mut World) {
    while let Some(packet) = world
        .resource_scope::<RenetClient, _>(|_, mut client| {
            //println!("Rtt: {}", client.rtt());
            client.receive_message(Channel::Replication)
        })
        .map(|msg| bincode::deserialize::<ReplicationPacket>(&msg).unwrap())
    {
        world.insert_resource(SyncedServerTick { tick: packet.tick });

        for despawn in packet.despawns {
            if let Some(local_entity) = world.resource::<NetworkEntities>().get(&despawn).copied() {
                world.despawn(local_entity);
            }
        }

        for EntityUpdates {
            entity,
            updates,
            removals,
        } in packet.updates
        {
            for removal in removals {
                world.resource_scope::<ReplicationFunctions, ()>(|world, f| {
                    let apply = &f[removal].remove;
                    apply(world, entity);
                })
            }
            for update in updates {
                world.resource_scope::<ReplicationFunctions, ()>(|world, f| {
                    let apply = &f[update.replication_id].update;
                    apply(world, entity, &update.data);
                })
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EntityUpdates {
    entity: Entity,
    updates: Vec<UpdateComponent>,
    removals: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EntityDespawns {
    entity: Entity,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateComponent {
    replication_id: usize,
    data: Vec<u8>,
}

struct ReplicationFunction {
    gather: Box<dyn Fn(&World, Entity) -> Option<Vec<u8>> + Send + Sync>,
    update: Box<dyn Fn(&mut World, Entity, &[u8]) + Send + Sync>,
    has_removed: Box<dyn Fn(&World, Entity) -> bool + Send + Sync>,
    remove: Box<dyn Fn(&mut World, Entity) + Send + Sync>,
}

#[derive(Resource, Deref, DerefMut, Default)]
struct ReplicationFunctions(Vec<ReplicationFunction>);

fn serialize_all_components(world: &World, entity: Entity) -> EntityUpdates {
    EntityUpdates {
        entity,
        updates: world
            .resource::<ReplicationFunctions>()
            .iter()
            .enumerate()
            .flat_map(|(replication_id, f)| {
                Some(UpdateComponent {
                    replication_id,
                    data: (f.gather)(world, entity)?,
                })
            })
            .collect(),
        removals: world
            .resource::<ReplicationFunctions>()
            .iter()
            .enumerate()
            .filter(|(_, f)| (f.has_removed)(world, entity))
            .map(|(replication_id, _)| replication_id)
            .collect(),
    }
}

// If any error is found we just panic
pub fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
    if let Some(e) = renet_error.read().next() {
        panic!("{}", e);
    }
}

// Implement convenience method on App
pub trait AppExt {
    fn replicate<T: Component + Serialize + for<'a> Deserialize<'a>>(&mut self) -> &mut Self;
    fn replicate_with<T: Component>(
        &mut self,
        gather: impl Fn(&T) -> Vec<u8> + Send + Sync + 'static,
        update: impl Fn(&[u8]) -> T + Send + Sync + 'static,
    ) -> &mut Self;
}

impl AppExt for App {
    fn replicate<T: Component + Serialize + for<'a> Deserialize<'a>>(&mut self) -> &mut Self {
        self.replicate_with::<T>(
            |component| bincode::serialize(component).unwrap(),
            |data| bincode::deserialize(data).unwrap(),
        )
    }

    fn replicate_with<T: Component>(
        &mut self,
        gather: impl Fn(&T) -> Vec<u8> + Send + Sync + 'static,
        update: impl Fn(&[u8]) -> T + Send + Sync + 'static,
    ) -> &mut Self {
        self.add_systems(
            NetworkResync,
            copy_replicated_component::<T>.in_set(CopyReplicated),
        );
        self.world
            .resource_mut::<ReplicationFunctions>()
            .push(ReplicationFunction {
                gather: Box::new(move |world, entity| {
                    let component = world.entity(entity).get::<T>()?;

                    Some(gather(component))
                }),
                update: Box::new(move |world, entity, data| {
                    let local_entity = world.resource::<NetworkEntities>().get(&entity).copied();

                    let component = Replicated(update(data));
                    match local_entity {
                        Some(local_entity) => {
                            if let Some(mut e) = world.get_entity_mut(local_entity) {
                                e.insert(component);
                            }
                        }
                        None => {
                            let local_entity = world.spawn(component).id();
                            world
                                .resource_mut::<NetworkEntities>()
                                .insert(entity, local_entity);
                        }
                    }
                }),
                has_removed: Box::new(move |world, entity| {
                    let Some(replicate_component_id) = world.component_id::<T>() else {
                        return false;
                    };

                    if let Some(events) = world.removed_components().get(replicate_component_id) {
                        for event in events.get_reader().read(events) {
                            if entity == (*event).clone().into() {
                                return true;
                            }
                        }
                    }

                    false
                }),
                remove: Box::new(move |world, entity| {
                    world.entity_mut(entity).remove::<T>();
                }),
            });
        self
    }
}

pub fn replication_connection_config() -> ConnectionConfig {
    let channels = vec![
        ChannelConfig {
            channel_id: Channel::Replication as u8,
            max_memory_usage_bytes: 5 * 1024 * 1024,
            send_type: SendType::ReliableOrdered {
                resend_time: Duration::from_millis(300),
            },
        },
        ChannelConfig {
            channel_id: Channel::ReliableOrdered as u8,
            max_memory_usage_bytes: 5 * 1024 * 1024,
            send_type: SendType::ReliableOrdered {
                resend_time: Duration::from_millis(300),
            },
        },
    ];

    ConnectionConfig {
        server_channels_config: channels.clone(),
        client_channels_config: channels,
        ..default()
    }
}

pub fn is_client(client: Option<Res<RenetClient>>) -> bool {
    client.is_some()
}

pub fn is_server(server: Option<Res<RenetServer>>) -> bool {
    server.is_some()
}
