use crate::replicate::*;
use crate::test_utils::*;

use super::NetworkUpdate;

#[test]
fn manual_tick() {
    #[derive(Resource)]
    struct TickCounter(u64);

    let mut server = create_server();
    let mut client = create_client(&mut server);
    for app in [&mut server, &mut client] {
        app.insert_resource(TickCounter(0)).add_systems(
            NetworkUpdate,
            |mut counter: ResMut<TickCounter>| {
                counter.0 += 1;
            },
        );
    }

    assert_eq!(server.world.resource::<NetworkTick>().0, 0);
    assert_eq!(server.world.resource::<TickCounter>().0, 0);
    assert_eq!(client.world.resource::<NetworkTick>().0, 0);
    assert_eq!(client.world.resource::<TickCounter>().0, 0);

    tick(&mut server);
    assert_eq!(server.world.resource::<NetworkTick>().0, 1);
    assert_eq!(server.world.resource::<TickCounter>().0, 1);
    assert_eq!(client.world.resource::<NetworkTick>().0, 0);
    assert_eq!(client.world.resource::<TickCounter>().0, 0);

    client.update();
    assert_eq!(server.world.resource::<NetworkTick>().0, 1);
    assert_eq!(server.world.resource::<TickCounter>().0, 1);
    assert_eq!(client.world.resource::<NetworkTick>().0, 1);
    assert_eq!(client.world.resource::<TickCounter>().0, 0);

    tick(&mut client);
    assert_eq!(server.world.resource::<NetworkTick>().0, 1);
    assert_eq!(server.world.resource::<TickCounter>().0, 1);
    assert_eq!(client.world.resource::<NetworkTick>().0, 2);
    assert_eq!(client.world.resource::<TickCounter>().0, 1);

    tick(&mut server);
    tick(&mut client);
    assert_eq!(server.world.resource::<NetworkTick>().0, 2);
    assert_eq!(server.world.resource::<TickCounter>().0, 2);
    assert_eq!(client.world.resource::<NetworkTick>().0, 3);
    assert_eq!(client.world.resource::<TickCounter>().0, 2);

    tick(&mut client);
    tick(&mut client);
    assert_eq!(server.world.resource::<NetworkTick>().0, 2);
    assert_eq!(server.world.resource::<TickCounter>().0, 2);
    assert_eq!(client.world.resource::<NetworkTick>().0, 5);
    assert_eq!(client.world.resource::<TickCounter>().0, 4);

    tick(&mut server);
    tick(&mut server);
    assert_eq!(server.world.resource::<NetworkTick>().0, 4);
    assert_eq!(server.world.resource::<TickCounter>().0, 4);
    assert_eq!(client.world.resource::<NetworkTick>().0, 5);
    assert_eq!(client.world.resource::<TickCounter>().0, 4);

    for _ in 0..10 {
        tick(&mut server);
    }
    tick(&mut client);
    assert_eq!(server.world.resource::<NetworkTick>().0, 14);
    assert_eq!(server.world.resource::<TickCounter>().0, 14);
    assert_eq!(client.world.resource::<NetworkTick>().0, 15);
    assert_eq!(client.world.resource::<TickCounter>().0, 5);

    for _ in 0..10 {
        tick(&mut client);
    }
    assert_eq!(server.world.resource::<NetworkTick>().0, 14);
    assert_eq!(server.world.resource::<TickCounter>().0, 14);
    assert_eq!(client.world.resource::<NetworkTick>().0, 25);
    assert_eq!(client.world.resource::<TickCounter>().0, 15);

    let starting_server_tick = server.world.resource::<NetworkTick>().0;
    let starting_server_counter = server.world.resource::<TickCounter>().0;
    let starting_client_tick = client.world.resource::<NetworkTick>().0;
    let starting_client_counter = client.world.resource::<TickCounter>().0;

    for i in 1..100 {
        tick(&mut server);
        tick(&mut client);
        assert_eq!(
            server.world.resource::<NetworkTick>().0,
            starting_server_tick + i
        );
        assert_eq!(
            server.world.resource::<TickCounter>().0,
            starting_server_counter + i
        );
        assert_eq!(
            client.world.resource::<NetworkTick>().0,
            starting_client_tick + i
        );
        assert_eq!(
            client.world.resource::<TickCounter>().0,
            starting_client_counter + 11 * i
        );
    }
}
