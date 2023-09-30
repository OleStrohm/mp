use std::time::Duration;

use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_renet::renet::{ChannelConfig, ConnectionConfig, RenetClient, RenetServer, SendType};
use serde::{Deserialize, Serialize};

use crate::shared::NetworkTick;

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

struct ReplicationPlugin;

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
            .add_systems(PreUpdate, receive_updated_components.run_if(is_client))
            .add_systems(PostUpdate, send_updated_components.run_if(is_server));
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ReplicationPacket {
    tick: NetworkTick,
    updates: Vec<UpdateComponent>,
}

fn send_updated_components(world: &mut World) {
    //, mut replicate: SystemState<Entity, With<Replicate>>) {
    // Create the list of updates
    let updates = world
        .query_filtered::<Entity, With<Replicate>>()
        .iter(world)
        .flat_map(|entity| serialize_all_components(world, entity))
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
        for message in packet.updates {
            let apply = world.resource::<ReplicationFunctions>()[message.replication_id].apply;
            apply(world, message);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateComponent {
    entity: Entity,
    replication_id: usize,
    data: Vec<u8>,
}

fn update_component<T: Component + for<'a> Deserialize<'a>>(
    world: &mut World,
    update: UpdateComponent,
) {
    let local_entity = world
        .resource::<NetworkEntities>()
        .get(&update.entity)
        .copied();

    let component = bincode::deserialize::<T>(&update.data).unwrap();
    match local_entity {
        Some(local_entity) => {
            world.entity_mut(local_entity).insert(component);
        }
        None => {
            let local_entity = world.spawn(component).id();
            world
                .resource_mut::<NetworkEntities>()
                .insert(update.entity, local_entity);
        }
    }
}

struct ReplicationFunction {
    gather: fn(&World, Entity, usize) -> Option<UpdateComponent>,
    apply: fn(&mut World, UpdateComponent),
}

#[derive(Resource, Deref, DerefMut, Default)]
struct ReplicationFunctions(Vec<ReplicationFunction>);

fn serialize_all_components(world: &World, entity: Entity) -> Vec<UpdateComponent> {
    world
        .resource::<ReplicationFunctions>()
        .iter()
        .enumerate()
        .flat_map(|(replication_id, f)| (f.gather)(world, entity, replication_id))
        .collect()
}

fn serialize_component<T: Component + Serialize>(
    world: &World,
    entity: Entity,
    replication_id: usize,
) -> Option<UpdateComponent> {
    let component = world.entity(entity).get::<T>()?;

    Some(UpdateComponent {
        entity,
        replication_id,
        data: bincode::serialize(component).unwrap(),
    })
}

// Implement convenience method on App
trait AppExt {
    fn replicate<T: Component + Serialize + for<'a> Deserialize<'a>>(&mut self) -> &mut Self;
}

impl AppExt for App {
    fn replicate<T: Component + Serialize + for<'a> Deserialize<'a>>(&mut self) -> &mut Self {
        self.world
            .resource_mut::<ReplicationFunctions>()
            .push(ReplicationFunction {
                gather: serialize_component::<T>,
                apply: update_component::<T>,
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

#[cfg(test)]
mod tests {
    use std::net::{SocketAddr, UdpSocket};
    use std::time::SystemTime;

    use bevy_renet::renet::transport::{
        ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
        ServerConfig,
    };
    use bevy_renet::transport::{NetcodeClientPlugin, NetcodeServerPlugin};
    use bevy_renet::{RenetClientPlugin, RenetServerPlugin};

    use crate::shared::PROTOCOL_ID;

    use super::*;

    #[derive(Debug, Serialize, Deserialize, Component)]
    struct ReplMarker;

    fn start_server_networking(
        mut commands: Commands,
        connection_config: Res<ReplicationConnectionConfig>,
    ) {
        let server = RenetServer::new(connection_config.0.clone());

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let public_addr = "127.0.0.1:5000".parse::<SocketAddr>().unwrap();
        let socket = UdpSocket::bind(public_addr).unwrap();
        let server_config = ServerConfig {
            max_clients: 64,
            protocol_id: PROTOCOL_ID,
            authentication: ServerAuthentication::Unsecure,
            public_addr,
        };

        let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

        commands.insert_resource(transport);
        commands.insert_resource(server);
    }

    fn start_client_networking(
        mut commands: Commands,
        connection_config: Res<ReplicationConnectionConfig>,
    ) {
        let client = RenetClient::new(connection_config.0.clone());

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let client_id = 0;
        let server_addr = "127.0.0.1:5000".parse::<SocketAddr>().unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let authentication = ClientAuthentication::Unsecure {
            client_id,
            protocol_id: PROTOCOL_ID,
            server_addr,
            user_data: None,
        };

        let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

        commands.insert_resource(transport);
        commands.insert_resource(client);
    }

    #[test]
    fn basic_repl() {
        let mut server = App::new();
        server
            .add_plugins((
                MinimalPlugins,
                ReplicationPlugin,
                RenetServerPlugin,
                NetcodeServerPlugin,
            ))
            .replicate::<ReplMarker>()
            .add_systems(Startup, start_server_networking)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn((Replicate, ReplMarker));
            });
        let mut client = App::new();
        client
            .add_plugins((
                MinimalPlugins,
                ReplicationPlugin,
                RenetClientPlugin,
                NetcodeClientPlugin,
            ))
            .replicate::<ReplMarker>()
            .add_systems(Startup, start_client_networking);

        server.update();
        client.update();

        assert!(server.world.get_resource::<RenetServer>().is_some());
        assert!(client.world.get_resource::<RenetClient>().is_some());

        while !client
            .world
            .resource::<NetcodeClientTransport>()
            .is_connected()
        {
            server.update();
            client.update();
        }

        server.update();
        client.update();

        let num_markers = client
            .world
            .query::<&ReplMarker>()
            .iter(&client.world)
            .count();

        assert!(num_markers == 1);
    }
}
