use rocket_ws as ws;
use std::collections::HashMap;
use std::sync::Arc;
use serde::Serialize;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

pub type Tx = mpsc::UnboundedSender<ws::Message>;

#[derive(Debug)]
pub struct WsAppState {
    pub clients: Mutex<Vec<Arc<Client>>>,
    pub rooms: Mutex<HashMap<String, Arc<Room>>>,
}

#[derive(Debug)]
pub struct Client {
    pub tx: Tx,
    pub uid: Uuid,
    pub data: Mutex<ClientData>,
}

#[derive(Debug)]
pub struct ClientData {
    pub name: Option<String>,
    pub room: Option<Arc<Room>>,
}

#[derive(Debug)]
pub struct Room {
    pub room_id: String,
    pub data: Mutex<RoomData>,
}

#[derive(Debug)]
pub struct RoomData {
    pub clients: Vec<RoomClient>,
}

#[derive(Debug)]
pub struct RoomClient {
    pub client: Arc<Client>,
    pub owner: bool,
    pub admin: bool,
}

impl WsAppState {
    pub fn new() -> Self {
        WsAppState {
            clients: Mutex::new(Vec::new()),
            rooms: Mutex::new(HashMap::new()),
        }
    }
}

impl Client {
    pub fn new(tx: Tx) -> Self {
        Client {
            tx,
            uid: Uuid::new_v4(),
            data: Mutex::new(ClientData {
                name: None,
                room: None,
            }),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        println!("Client: drop");
    }
}

impl Room {
    pub fn new(room_id: String) -> Self {
        Room {
            room_id,
            data: Mutex::new(RoomData {
                clients: Vec::new(),
            }),
        }
    }

    pub fn new_with_owner(room_id: String, client: Arc<Client>) -> Self {
        Room {
            room_id,
            data: Mutex::new(RoomData {
                clients: vec![RoomClient {
                    client,
                    owner: true,
                    admin: true,
                }],
            }),
        }
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        println!("Room: drop");
    }
}

impl RoomData {
    pub fn add_client(&mut self, client: Arc<Client>) {
        self.clients.push(RoomClient {
            client,
            owner: false,
            admin: false,
        })
    }

    pub fn remove_client(&mut self, client: &Arc<Client>) {
        let index = self
            .clients
            .iter()
            .position(|x| Arc::ptr_eq(&x.client, client))
            .unwrap();

        let owner_left = self.clients[index].owner;

        self.clients.remove(index);

        if owner_left && !self.clients.is_empty() {
            self.clients[0].owner = true;
        }
    }

    pub fn can_control(&self, client: Arc<Client>) -> bool {
        let room_client = self.clients.iter().find(|c| Arc::ptr_eq(&c.client, &client));
        if let Some(room_client) = room_client {
            room_client.can_control()
        } else {
            false
        }
    }
}

impl RoomClient {
    pub fn can_control(&self) -> bool {
        self.owner || self.admin
    }
}