use std::time::Duration;

use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::{ChannelConfig, ConnectionConfig, RenetClient, RenetServer, SendType};
use bevy_renet::transport::{NetcodeClientPlugin, NetcodeServerPlugin};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

pub const PROTOCOL_ID: u64 = 7;

#[derive(Resource, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct NetworkTick(pub u64);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct NetworkEntities(HashMap<Entity, Entity>);

#[derive(Component, Clone, Copy)]
pub struct Replicate;

#[repr(u8)]
pub enum Channel {
    Replication = 0,
    ReliableOrdered,
}

impl From<Channel> for u8 {
    fn from(channel: Channel) -> Self {
        channel as u8
    }
}

#[derive(Resource, Deref, DerefMut, Default, Clone)]
pub struct ReplicationConnectionConfig(pub ConnectionConfig);

pub struct ReplicationPlugin;

impl Plugin for ReplicationPlugin {
    fn build(&self, app: &mut App) {
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
        let connection_config = ConnectionConfig {
            server_channels_config: channels.clone(),
            client_channels_config: channels,
            ..default()
        };

        app.init_resource::<ReplicationFunctions>()
            .init_resource::<NetworkTick>()
            .init_resource::<NetworkEntities>()
            .insert_resource(ReplicationConnectionConfig(connection_config))
            .add_systems(
                PreUpdate,
                receive_updated_components
                    .after(NetcodeClientPlugin::update_system)
                    .run_if(is_client),
            )
            .add_systems(
                PostUpdate,
                send_updated_components
                    .before(NetcodeServerPlugin::send_packets)
                    .run_if(is_server),
            );
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ReplicationPacket {
    tick: NetworkTick,
    updates: Vec<EntityUpdates>,
}

fn send_updated_components(world: &mut World) {
    // Create the list of updates
    let updates = world
        .query_filtered::<Entity, With<Replicate>>()
        .iter(world)
        .map(|entity| serialize_all_components(world, entity))
        .collect();
    let tick = *world.resource::<NetworkTick>();

    // send it in a REPLICATION_CHANNEL
    let packet = ReplicationPacket { tick, updates };
    let mut server = world.resource_mut::<RenetServer>();

    server.broadcast_message(Channel::Replication, bincode::serialize(&packet).unwrap());
}

fn receive_updated_components(world: &mut World) {
    // receive from REPLICATION_CHANNEL

    let packet = world
        .resource_scope::<RenetClient, _>(|_, mut client| {
            client.receive_message(Channel::Replication)
        })
        .map(|msg| bincode::deserialize::<ReplicationPacket>(&msg).unwrap());

    // Create the list of updates
    if let Some(packet) = packet {
        for EntityUpdates { entity, updates } in packet.updates {
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
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateComponent {
    replication_id: usize,
    data: Vec<u8>,
}

fn update_component<T: Component + for<'a> Deserialize<'a>>(
    world: &mut World,
    entity: Entity,
    data: &[u8],
) {
    let local_entity = world.resource::<NetworkEntities>().get(&entity).copied();

    let component = bincode::deserialize::<T>(data).unwrap();
    match local_entity {
        Some(local_entity) => {
            world.entity_mut(local_entity).insert(component);
        }
        None => {
            let local_entity = world.spawn(component).id();
            world
                .resource_mut::<NetworkEntities>()
                .insert(entity, local_entity);
        }
    }
}

struct ReplicationFunction {
    gather: Box<dyn Fn(&World, Entity) -> Option<Vec<u8>> + Send + Sync>,
    update: Box<dyn Fn(&mut World, Entity, &[u8]) + Send + Sync>,
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
    }
}

fn gather_component<T: Component + Serialize>(world: &World, entity: Entity) -> Option<Vec<u8>> {
    let component = world.entity(entity).get::<T>()?;

    Some(bincode::serialize(component).unwrap())
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
        self.world
            .resource_mut::<ReplicationFunctions>()
            .push(ReplicationFunction {
                gather: Box::new(gather_component::<T>),
                update: Box::new(update_component::<T>),
            });
        self
    }

    fn replicate_with<T: Component>(
        &mut self,
        gather: impl Fn(&T) -> Vec<u8> + Send + Sync + 'static,
        update: impl Fn(&[u8]) -> T + Send + Sync + 'static,
    ) -> &mut Self {
        self.world
            .resource_mut::<ReplicationFunctions>()
            .push(ReplicationFunction {
                gather: Box::new(move |world, entity| {
                    let component = world.entity(entity).get::<T>()?;

                    Some(gather(component))
                }),
                update: Box::new(move |world, entity, data| {
                    let local_entity = world.resource::<NetworkEntities>().get(&entity).copied();

                    let component = update(data);
                    match local_entity {
                        Some(local_entity) => {
                            world.entity_mut(local_entity).insert(component);
                        }
                        None => {
                            let local_entity = world.spawn(component).id();
                            world
                                .resource_mut::<NetworkEntities>()
                                .insert(entity, local_entity);
                        }
                    }
                }),
            });
        self
    }
}

fn is_client(client: Option<Res<RenetClient>>) -> bool {
    client.is_some()
}

fn is_server(server: Option<Res<RenetServer>>) -> bool {
    server.is_some()
}
