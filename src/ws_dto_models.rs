use rocket::futures::future::join_all;
use rocket::serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::ws_app_state::{RoomClient, RoomData};

#[derive(Serialize, Deserialize, Debug)]
pub struct RoomDataDto {
    pub clients: Vec<RoomClientDto>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RoomClientDto {
    pub name: Option<String>,
    pub uid: Uuid,
    pub owner: bool,
    pub admin: bool,
}

impl RoomDataDto {
    pub async fn from(value: &RoomData) -> Self {
        RoomDataDto {
            clients: join_all(value.clients.iter().map(RoomClientDto::from)).await,
        }
    }
}

impl RoomClientDto {
    pub async fn from(value: &RoomClient) -> Self {
        RoomClientDto {
            name: value.client.data.lock().await.name.clone(),
            uid: value.client.uid,
            owner: value.owner,
            admin: value.admin,
        }
    }
}