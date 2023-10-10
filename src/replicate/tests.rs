use bevy_renet::{RenetClientPlugin, RenetServerPlugin};

use crate::transport::{MemoryClientPlugin, MemoryServerPlugin, MemoryServerTransport};

use super::*;

#[derive(Debug, Serialize, Deserialize, Component)]
struct ReplMarker;

#[derive(Debug, Serialize, Deserialize, Component)]
struct ReplMarker2;

#[derive(Debug, Serialize, Deserialize, Component, PartialEq, Eq, Clone, Copy)]
struct ReplNum(u32);

fn create_apps() -> (App, App) {
    let mut server = App::new();
    server
        .add_plugins((
            MinimalPlugins,
            RenetServerPlugin,
            MemoryServerPlugin,
            ReplicationPlugin::new(0.01, TickStrategy::Manual),
        ))
        .insert_resource(TickStrategy::Manual);

    let mut client = App::new();
    client
        .add_plugins((
            MinimalPlugins,
            RenetClientPlugin,
            MemoryClientPlugin,
            ReplicationPlugin::new(0.01, TickStrategy::Manual),
        ))
        .insert_resource(TickStrategy::Manual);

    (server, client)
}

fn connect(server: &mut App, client: &mut App) {
    let connection_config = server
        .world
        .resource::<ReplicationConnectionConfig>()
        .clone();

    let mut server_transport = MemoryServerTransport::default();
    let client_transport = server_transport.create_client();

    let renet_server = RenetServer::new(connection_config.0.clone());
    let renet_client = RenetClient::new(connection_config.0);

    server
        .insert_resource(renet_server)
        .insert_resource(server_transport);
    client
        .insert_resource(renet_client)
        .insert_resource(client_transport);

    update(server, client);
}

fn update(server: &mut App, client: &mut App) {
    server.update();
    client.update();
}

#[test]
fn basic_repl() {
    let (mut server, mut client) = create_apps();
    server.replicate::<ReplMarker>();
    client.replicate::<ReplMarker>();

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
    assert_eq!(marker_num, ReplNum(0));

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
    assert_eq!(marker_num, ReplNum(1));

    assert_eq!(first_marked_entity, second_marked_entity);
}

#[test]
fn stress_100() {
    for _ in 0..100 {
        modified_same_entity();
    }
}

#[test]
fn replicate_transform() {
    let (mut server, mut client) = create_apps();
    for app in [&mut server, &mut client] {
        app.replicate_with::<Transform>(
            |component| bincode::serialize(&component.translation).unwrap(),
            |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
        );
    }

    // Spawn one replicated component
    server
        .world
        .spawn((Replicate, Transform::from_xyz(1.0, 2.0, 3.0)));

    connect(&mut server, &mut client);

    update(&mut server, &mut client);

    let tf = server.world.query::<&Transform>().single(&server.world);
    assert_eq!(Vec3::new(1.0, 2.0, 3.0), tf.translation);
}
