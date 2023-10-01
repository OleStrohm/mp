use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::SystemTime;

use bevy_renet::renet::transport::{
    ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
    ServerConfig,
};
use bevy_renet::transport::{NetcodeClientPlugin, NetcodeServerPlugin};
use bevy_renet::{RenetClientPlugin, RenetServerPlugin};

use super::*;

#[derive(Debug, Serialize, Deserialize, Component)]
struct ReplMarker;

#[derive(Debug, Serialize, Deserialize, Component)]
struct ReplMarker2;

#[derive(Debug, Serialize, Deserialize, Component, PartialEq, Eq, Clone, Copy)]
struct ReplNum(u32);

fn start_server_networking(
    connection_config: ReplicationConnectionConfig,
) -> (RenetServer, NetcodeServerTransport) {
    let server = RenetServer::new(connection_config.0);

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0);
    let socket = UdpSocket::bind(server_addr).unwrap();
    let public_addr = socket.local_addr().unwrap();
    let server_config = ServerConfig {
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        authentication: ServerAuthentication::Unsecure,
        public_addr,
    };

    let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

    (server, transport)
}

fn start_client_networking(
    server_port: u16,
    connection_config: ReplicationConnectionConfig,
) -> (RenetClient, NetcodeClientTransport) {
    let client = RenetClient::new(connection_config.0);

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let client_id = 0;
    let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), server_port);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr,
        user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    (client, transport)
}

fn create_apps() -> (App, App) {
    let mut server = App::new();
    server.add_plugins((
        MinimalPlugins,
        RenetServerPlugin,
        NetcodeServerPlugin,
        ReplicationPlugin,
    ));
    let mut client = App::new();
    client.add_plugins((
        MinimalPlugins,
        RenetClientPlugin,
        NetcodeClientPlugin,
        ReplicationPlugin,
    ));

    (server, client)
}

fn connect(server: &mut App, client: &mut App) {
    let connection_config = server
        .world
        .resource::<ReplicationConnectionConfig>()
        .clone();

    let (renet_server, server_transport) = start_server_networking(connection_config.clone());
    let server_port = server_transport.addr().port();
    server
        .insert_resource(renet_server)
        .insert_resource(server_transport);
    let (renet_client, client_transport) = start_client_networking(server_port, connection_config);
    client
        .insert_resource(renet_client)
        .insert_resource(client_transport);
    server.update();
    client.update();

    while !client
        .world
        .resource::<NetcodeClientTransport>()
        .is_connected()
    {
        server.update();
        client.update();
    }
}

/// Updates the server and the client once
fn update(server: &mut App, client: &mut App) {
    server.update();
    client.update();
}

#[test]
fn basic_repl() {
    let (mut server, mut client) = create_apps();
    server.replicate::<ReplMarker>();
    client.replicate::<ReplMarker>();

    // Spawn one replicated component
    server.world.spawn((Replicate, ReplMarker));

    connect(&mut server, &mut client);

    update(&mut server, &mut client);

    let num_markers = client
        .world
        .query::<&ReplMarker>()
        .iter(&client.world)
        .count();

    assert!(num_markers == 1);
}

#[test]
fn multiple_repl() {
    let (mut server, mut client) = create_apps();
    for app in [&mut server, &mut client] {
        app.replicate::<ReplMarker>().replicate::<ReplMarker2>();
    }

    // Spawn one replicated component
    server.world.spawn((Replicate, ReplMarker));
    server.world.spawn((Replicate, ReplMarker2));
    server.world.spawn((Replicate, ReplMarker2));
    server.world.spawn((Replicate, ReplMarker, ReplMarker2));

    connect(&mut server, &mut client);

    update(&mut server, &mut client);

    let num_markers_1 = client
        .world
        .query::<&ReplMarker>()
        .iter(&client.world)
        .count();
    let num_markers_2 = client
        .world
        .query::<&ReplMarker2>()
        .iter(&client.world)
        .count();
    let num_markers_both = client
        .world
        .query::<(&ReplMarker, &ReplMarker2)>()
        .iter(&client.world)
        .count();

    assert!(num_markers_1 == 2);
    assert!(num_markers_2 == 3);
    assert!(num_markers_both == 1);
}

#[test]
fn modified_same_entity() {
    let (mut server, mut client) = create_apps();
    for app in [&mut server, &mut client] {
        app.replicate::<ReplNum>();
    }

    // Spawn one replicated component
    server.world.spawn((Replicate, ReplNum(0)));

    connect(&mut server, &mut client);

    update(&mut server, &mut client);

    let (first_marked_entity, &marker_num) = client
        .world
        .query::<(Entity, &ReplNum)>()
        .single(&client.world);
    assert_eq!(ReplNum(0), marker_num);

    for mut num in server
        .world
        .query::<&mut ReplNum>()
        .iter_mut(&mut server.world)
    {
        num.0 = 1;
    }

    update(&mut server, &mut client);
    update(&mut server, &mut client);

    let (second_marked_entity, &marker_num) = client
        .world
        .query::<(Entity, &ReplNum)>()
        .single(&client.world);
    assert_eq!(ReplNum(1), marker_num);

    assert_eq!(first_marked_entity, second_marked_entity);
}

#[test]
fn stress_100() {
    for _ in 0..100 {
        let (mut server, mut client) = create_apps();
        for app in [&mut server, &mut client] {
            app.replicate::<ReplNum>();
        }

        // Spawn one replicated component
        server.world.spawn((Replicate, ReplNum(0)));

        connect(&mut server, &mut client);

        update(&mut server, &mut client);

        let (first_marked_entity, &marker_num) = client
            .world
            .query::<(Entity, &ReplNum)>()
            .single(&client.world);
        assert_eq!(ReplNum(0), marker_num);

        for mut num in server
            .world
            .query::<&mut ReplNum>()
            .iter_mut(&mut server.world)
        {
            num.0 = 1;
        }

        update(&mut server, &mut client);

        let (second_marked_entity, &marker_num) = client
            .world
            .query::<(Entity, &ReplNum)>()
            .single(&client.world);
        assert_eq!(ReplNum(1), marker_num);

        assert_eq!(first_marked_entity, second_marked_entity);
    }
}
