use std::time::Duration;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_renet::renet::RenetClient;
use itertools::Itertools;

use crate::prediction::{is_desynced, Resimulating};
use crate::replicate::{NetworkTick, SyncedServerTick};

use super::Replicate;

#[cfg(test)]
mod tests;

#[derive(Debug, Resource, Deref, DerefMut)]
pub struct NetworkFixedTime(pub FixedTime);

#[derive(Resource)]
pub struct DoTick;

#[derive(Default, Resource, PartialEq, Eq, Clone, Copy)]
pub enum TickStrategy {
    #[default]
    Automatic,
    #[allow(unused)]
    Manual,
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkResync;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkBlueprint;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkUpdateTick;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkPreUpdate;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkUpdate;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkPostUpdate;

#[derive(Resource, Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct NetworkScheduleOrder {
    pub labels: Vec<Box<dyn ScheduleLabel>>,
}

impl Default for NetworkScheduleOrder {
    fn default() -> Self {
        Self {
            labels: vec![
                Box::new(NetworkUpdateTick),
                Box::new(NetworkBlueprint),
                Box::new(NetworkPreUpdate),
                Box::new(NetworkUpdate),
                Box::new(NetworkPostUpdate),
            ],
        }
    }
}

pub(super) fn run_network_fixed(world: &mut World) {
    if *world.resource::<TickStrategy>() == TickStrategy::Automatic {
        let delta_time = world.resource::<Time>().delta();
        world.resource_mut::<NetworkFixedTime>().tick(delta_time);

        if world.get_resource::<RenetClient>().is_some()
            && world.get_resource::<NetworkTick>().is_some()
            && world.is_resource_changed::<SyncedServerTick>()
        {
            let last_received_server_tick = world.resource::<SyncedServerTick>().tick.0;
            let current_tick = world.resource::<NetworkTick>().0;
            let rtt = world.resource::<RenetClient>().rtt();
            let period = world
                .resource_mut::<NetworkFixedTime>()
                .period
                .as_secs_f64();
            let ahead_by = 4.0 * rtt;
            let speed_up =
                (last_received_server_tick as f64 - current_tick as f64) * period + ahead_by;
            //let current_elapsed = world.resource::<Time>().elapsed_seconds_f64();
            //let should_be = current_elapsed + speed_up;
            //println!(
            //    "Tick {}: Last server tick ({}), and rtt is {:?}, so client should be {} ticks ahead",
            //    current_tick,
            //    last_received_server_tick,
            //    rtt,
            //    ahead_by / period,
            //);
            //println!(
            //    "elapes is currently {current_elapsed}, but it should be {should_be}, so speeding it up by {speed_up}"
            //);

            world
                .resource_mut::<NetworkFixedTime>()
                .tick(Duration::from_secs_f64(speed_up.clamp(0.0, 2.0 * period)));
        }
    }

    world.resource_scope(|world, order: Mut<NetworkScheduleOrder>| {
        if world.is_resource_changed::<SyncedServerTick>() && is_desynced(world) {
            let current_tick = *world.resource::<NetworkTick>();
            let synced_server_tick = world.resource::<SyncedServerTick>().tick;

            world.run_schedule(NetworkResync);

            if current_tick > synced_server_tick {
                //println!("Resimulating from {synced_server_tick:?} to {current_tick:?}");

                *world.resource_mut::<NetworkTick>() = synced_server_tick;

                let predicted_spawns = world
                    .query_filtered::<Entity, With<Replicate>>()
                    .iter_mut(world)
                    .collect_vec();
                for entity in predicted_spawns {
                    world.despawn(entity);
                }

                world.init_resource::<Resimulating>();
                while *world.resource::<NetworkTick>() != current_tick {
                    for label in &order.labels {
                        let _ = world.try_run_schedule(&**label);
                    }
                }
                world.remove_resource::<Resimulating>();
            }
        }

        while should_run(world) {
            for label in &order.labels {
                let _ = world.try_run_schedule(&**label);
            }
        }
    });
}

fn should_run(world: &mut World) -> bool {
    match *world.resource::<TickStrategy>() {
        TickStrategy::Automatic => world.resource_mut::<NetworkFixedTime>().expend().is_ok(),
        TickStrategy::Manual => world.remove_resource::<DoTick>().is_some(),
    }
}
