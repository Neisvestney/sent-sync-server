#[macro_use]
extern crate rocket;
mod ws_handler;
mod ws_app_state;
mod ws_dto_models;

use crate::ws_app_state::WsAppState;
use std::sync::Arc;

#[launch]
fn rocket() -> _ {
    let state = Arc::new(WsAppState::new());
    
    rocket::build()
        .manage(state)
        .mount("/", routes![ws_handler::ws_handler])
}
