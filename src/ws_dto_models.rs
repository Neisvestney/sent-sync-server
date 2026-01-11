use rocket::futures::future::join_all;
use rocket::serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;
use crate::ws_app_state::{RoomClient, RoomData};

#[derive(Serialize, Deserialize, Debug, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomDataDto {
    pub clients: Vec<RoomClientDto>,
    pub page_url: Option<String>,
    pub allow_stop_due_to_video_loading: bool,
}

#[derive(Serialize, Deserialize, Debug, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomClientDto {
    pub name: Option<String>,
    #[ts(type = "string")]
    pub uid: Uuid,
    pub owner: bool,
    pub admin: bool,
}

impl RoomDataDto {
    pub async fn from(value: &RoomData) -> Self {
        RoomDataDto {
            clients: join_all(value.clients.iter().map(RoomClientDto::from)).await,
            page_url: value.page_url.clone(),
            allow_stop_due_to_video_loading: value.allow_stop_due_to_video_loading,
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