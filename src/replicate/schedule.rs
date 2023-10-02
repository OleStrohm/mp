use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

#[derive(Debug, Resource, Deref, DerefMut)]
pub struct NetworkFixedTime(pub FixedTime);

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkResimulate;
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
                Box::new(NetworkResimulate),
                Box::new(NetworkBlueprint),
                Box::new(NetworkPreUpdate),
                Box::new(NetworkUpdate),
                Box::new(NetworkPostUpdate),
            ],
        }
    }
}

pub(super) fn run_network_fixed(world: &mut World) {
    let delta_time = world.resource::<Time>().delta();
    let mut fixed_time = world.resource_mut::<NetworkFixedTime>();
    fixed_time.tick(delta_time);

    world.resource_scope(|world, order: Mut<NetworkScheduleOrder>| {
        while world.resource_mut::<NetworkFixedTime>().expend().is_ok() {
            for label in &order.labels {
                let _ = world.try_run_schedule(&**label);
            }
        }
    });
}
