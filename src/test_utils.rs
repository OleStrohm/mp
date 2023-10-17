use bevy::prelude::*;
use bevy_renet::renet::{RenetClient, RenetServer};

use crate::replicate::schedule::{DoTick, TickStrategy};
use crate::replicate::{replication_connection_config, ReplicationPlugin};
use crate::transport::{MemoryClientPlugin, MemoryServerPlugin, MemoryServerTransport};

pub fn create_server() -> App {
    let mut server = App::new();

    let server_transport = MemoryServerTransport::default();
    let renet_server = RenetServer::new(replication_connection_config());

    server
        .add_plugins((
            MinimalPlugins,
            MemoryServerPlugin,
            ReplicationPlugin::new(0.01, TickStrategy::Manual),
        ))
        .insert_resource(renet_server)
        .insert_resource(server_transport);

    server
}

pub fn create_client(server: &mut App) -> App {
    let mut client = App::new();

    let client_transport = server
        .world
        .resource_mut::<MemoryServerTransport>()
        .create_client();
    let renet_client = RenetClient::new(replication_connection_config());

    client
        .add_plugins((
            MinimalPlugins,
            MemoryClientPlugin,
            ReplicationPlugin::new(0.01, TickStrategy::Manual),
        ))
        .insert_resource(renet_client)
        .insert_resource(client_transport);

    client
}

pub fn tick(app: &mut App) {
    app.insert_resource(DoTick);
    app.update();
}
