use crate::test_utils::*;

use super::*;

#[derive(Debug, Serialize, Deserialize, Component)]
struct Marker;

#[derive(Debug, Serialize, Deserialize, Component)]
struct Marker2;

#[derive(Debug, Serialize, Deserialize, Component, PartialEq, Eq, Clone, Copy)]
struct Num(u32);

#[test]
fn basic_repl() {
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Marker>();
    }

    server.world.spawn((Replicate, Marker));

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 1);
}

#[test]
fn multiple_repl() {
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Marker>().replicate::<Marker2>();
    }

    // Spawn one replicated component
    server.world.spawn((Replicate, Marker));
    server.world.spawn((Replicate, Marker2));
    server.world.spawn((Replicate, Marker2));
    server.world.spawn((Replicate, Marker, Marker2));

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 2);
    assert_eq!(count::<&Marker2>(&mut client), 3);
    assert_eq!(count::<(&Marker, &Marker2)>(&mut client), 1);
}

#[test]
fn modified_same_entity() {
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Num>();
    }

    // Spawn one replicated component
    server.world.spawn((Replicate, Num(0)));

    server.update();
    client.update();

    let (first_marked_entity, &marker_num) =
        client.world.query::<(Entity, &Num)>().single(&client.world);
    assert_eq!(marker_num, Num(0));

    for mut num in server.world.query::<&mut Num>().iter_mut(&mut server.world) {
        num.0 = 1;
    }

    server.update();
    client.update();

    let (second_marked_entity, &marker_num) =
        client.world.query::<(Entity, &Num)>().single(&client.world);
    assert_eq!(marker_num, Num(1));

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
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate_with::<Transform>(
            |component| bincode::serialize(&component.translation).unwrap(),
            |data| Transform::from_translation(bincode::deserialize::<Vec3>(data).unwrap()),
        );
    }

    server
        .world
        .spawn((Replicate, Transform::from_xyz(1.0, 2.0, 3.0)));

    server.update();
    client.update();

    let tf = server.world.query::<&Transform>().single(&server.world);
    assert_eq!(Vec3::new(1.0, 2.0, 3.0), tf.translation);
}

#[test]
fn remove_component() {
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Marker>();
    }

    let marker = server.world.spawn((Replicate, Marker)).id();

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 1);

    server.world.entity_mut(marker).remove::<Marker>();

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 0);

    server.world.entity_mut(marker).insert(Marker);

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 1);
}

#[test]
fn despawn_entity() {
    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Marker>();
    }

    let marker = server.world.spawn((Replicate, Marker)).id();

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 1);

    server.world.despawn(marker);

    server.update();
    client.update();

    assert_eq!(count::<&Marker>(&mut client), 0);
}
