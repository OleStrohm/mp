use bevy::prelude::*;
use bevy_renet::renet::transport::NetcodeTransportError;

pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

pub struct SharedPlugin;

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, panic_on_error_system);
        //.insert_resource(FixedTime::new_from_secs(FIXED_TIMESTEP));
    }
}

// If any error is found we just panic
pub fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
    for e in renet_error.iter() {
        panic!("{}", e);
    }
}
