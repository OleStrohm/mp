use bevy::reflect::TypePath;

use crate::replicate::schedule::*;
use crate::replicate::*;
use crate::test_utils::{count, tick};

use super::*;

fn create_server<A>() -> App
where
    A: Actionlike + Serialize + for<'a> Deserialize<'a> + Send + Sync + 'static,
{
    let mut server = crate::test_utils::create_server();
    server.add_plugins(PredictionPlugin::<A>::default());

    server
}

fn create_client<A>(server: &mut App) -> App
where
    A: Actionlike + Serialize + for<'a> Deserialize<'a> + Send + Sync + 'static,
{
    let mut client = crate::test_utils::create_client(server);
    client.add_plugins(PredictionPlugin::<A>::default());

    client
}

#[test]
fn basic_prediction() {
    #[derive(Component, Serialize, Deserialize)]
    struct Pos(u64);

    #[derive(Actionlike, Clone, Copy, TypePath, Serialize, Deserialize)]
    enum NoAction {}

    let mut server = create_server::<NoAction>();
    let mut client = create_client::<NoAction>(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Pos>()
            .add_systems(NetworkUpdate, |mut positions: Query<&mut Pos>| {
                for mut pos in &mut positions {
                    pos.0 += 1;
                }
            });
    }

    server.world.spawn((Replicate, Pos(0)));

    tick(&mut server);
    client.update();

    for pos in server.world.query::<&Pos>().iter(&server.world) {
        assert_eq!(pos.0, 1);
    }
    for pos in client.world.query::<&Pos>().iter(&client.world) {
        assert_eq!(pos.0, 1);
    }

    tick(&mut client);
    tick(&mut client);

    for pos in server.world.query::<&Pos>().iter(&server.world) {
        assert_eq!(pos.0, 1);
    }
    for pos in client.world.query::<&Pos>().iter(&client.world) {
        assert_eq!(pos.0, 3);
    }

    tick(&mut server);
    tick(&mut client);

    for pos in server.world.query::<&Pos>().iter(&server.world) {
        assert_eq!(pos.0, 2);
    }
    for pos in client.world.query::<&Pos>().iter(&client.world) {
        assert_eq!(pos.0, 4);
    }
}

#[test]
fn predicted_spawn() {
    #[derive(Component, Serialize, Deserialize)]
    struct Player;
    #[derive(Component, Serialize, Deserialize)]
    struct Marker;

    #[derive(Actionlike, Clone, Copy, TypePath, Serialize, Deserialize)]
    enum Action {
        Spawn,
    }

    let mut server = create_server::<Action>();
    let mut client = create_client::<Action>(&mut server);
    for app in [&mut server, &mut client] {
        app.replicate::<Player>().replicate::<Marker>().add_systems(
            NetworkUpdate,
            |mut commands: Commands, players: Query<(Option<&Replicate>, &ActionState<Action>)>| {
                for (has_replicate, actions) in &players {
                    println!("checking actions for {}", has_replicate.is_some());
                    if actions.pressed(Action::Spawn) {
                        commands.spawn((Replicate, Marker));
                    }
                }
            },
        );
    }

    server.world.spawn((Replicate, Player));
    tick(&mut client);
    tick(&mut server);
    tick(&mut client);

    let player = client
        .world
        .query_filtered::<Entity, With<Player>>()
        .iter(&client.world)
        .next()
        .unwrap();
    client.world.entity_mut(player).insert(Control);

    let mut actions = ActionState::<Action>::default();
    actions.press(Action::Spawn);
    client.world.entity_mut(player).insert(actions);
    tick(&mut client);
    assert_eq!(count::<&Marker>(&mut client), 1);

    let actions = ActionState::<Action>::default();
    client.world.entity_mut(player).insert(actions);
    tick(&mut client);
    assert_eq!(count::<&Marker>(&mut client), 1);
    assert_eq!(count::<&Marker>(&mut server), 0);

    tick(&mut server);
    tick(&mut client);
    tick(&mut server);
    tick(&mut client);
    tick(&mut server);
    tick(&mut client);
    assert_eq!(count::<&Marker>(&mut server), 1);

    tick(&mut client);
    assert_eq!(count::<&Marker>(&mut client), 1);

    tick(&mut server);
    tick(&mut client);
    assert_eq!(count::<&Marker>(&mut client), 1);
}
