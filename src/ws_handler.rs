use std::ops::{Deref, DerefMut};
use std::sync::{Arc};
use rocket::futures::{SinkExt, StreamExt};
use rocket::serde::{Deserialize, Serialize};
use rocket::State;
use tokio::sync::{mpsc, MutexGuard};
use rocket_ws as ws;
use rocket_ws::Message;
use tokio::sync::mpsc::error::SendError;
use uuid::Uuid;
use crate::ws_app_state::{Client, ClientData, Room, RoomClient, RoomData, WsAppState};
use crate::ws_dto_models::{RoomDataDto};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum IncomingMessage {
    Ping,
    ChangeName { new_name: String },
    JoinRoom { room_id: String },
    ChangeClientAdminStatus { client_uid: Uuid, admin: bool },
    QuitRoom,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum OutgoingMessage {
    Pong,
    ClientUid { client_uid: Uuid },
    Success,
    Error { kind: ErrorKind, msg: Option<String> },
    RoomChanged { data: RoomDataDto },
}

#[derive(Serialize, Deserialize, Debug)]
enum ErrorKind {
    JsonError,
    ClientNotInAnyRoom,
    ClientNameNotSet,
    ClientNameTooShort,
    RoomIdTooShort,
    NoSuchClient,
    Forbidden,
}

#[get("/ws")]
pub fn ws_handler(ws: ws::WebSocket, state: &State<Arc<WsAppState>>) -> ws::Channel<'static> {
    let state = state.inner().clone();

    ws.channel(move|stream| {
        Box::pin(async move {
            let (mut sink, mut stream) = stream.split();
            // Create a channel for this client
            let (tx, mut rx) = mpsc::unbounded_channel::<ws::Message>();
            // Register this client
            let current_client = Arc::new(Client::new(tx.clone()));
            state.clients.lock().await.push(current_client.clone());

            // spawn a task for outgoing messages to this client
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if let Err(e) = sink.send(msg).await {
                        eprintln!("send error: {:?}", e);
                        break;
                    }
                }
            });

            response_with_json(&current_client, OutgoingMessage::ClientUid {client_uid: current_client.uid});

            // handle incoming messages
            while let Some(Ok(msg)) = stream.next().await {
                if let ws::Message::Text(txt) = msg {
                    match serde_json::from_str::<IncomingMessage>(&txt) {
                        Ok(inc) => {
                            match inc {
                                IncomingMessage::Ping => {
                                    response_with_json(&current_client, OutgoingMessage::Pong)
                                }
                                IncomingMessage::ChangeName { new_name } => 'label: {
                                    if new_name.len() <= 2 {
                                        response_with_error(&current_client, ErrorKind::ClientNameTooShort);
                                        break 'label;
                                    }

                                    let mut client_data = current_client.data.lock().await;
                                    client_data.name = Some(new_name);
                                    response_with_success(&current_client);
                                    if client_data.room.is_some() {
                                        let room = client_data.room.as_ref().unwrap().clone();
                                        drop(client_data);
                                        broadcast_room_change(&room.data.lock().await.deref()).await;
                                    }
                                }
                                IncomingMessage::JoinRoom { room_id } => 'label: {
                                    if !validate_client_name(&current_client).await {
                                        break 'label;
                                    }

                                    if room_id.len() <= 2 {
                                        response_with_error(&current_client, ErrorKind::RoomIdTooShort);
                                        break 'label;
                                    }

                                    let mut rooms = state.rooms.lock().await;
                                    if let Some(room) = rooms.get_mut(&room_id) {
                                        // Join existing room
                                        room.data.lock().await.add_client(current_client.clone());
                                        current_client.data.lock().await.room = Some(room.clone());

                                        response_with_success(&current_client);
                                        broadcast_room_change(&room.data.lock().await.deref()).await;
                                    } else {
                                        // Create new one
                                        let new_room = Room::new_with_owner(room_id.clone(), current_client.clone());
                                        let new_room = Arc::new(new_room);
                                        current_client.data.lock().await.room = Some(new_room.clone());

                                        response_with_success(&current_client);
                                        broadcast_room_change(&new_room.data.lock().await.deref()).await;

                                        rooms.insert(room_id, new_room);
                                    }
                                }
                                IncomingMessage::ChangeClientAdminStatus { client_uid, admin } => {
                                    if let Ok(current_client_data) = client_in_room(&current_client).await {
                                        let room = current_client_data.room.as_ref().unwrap().clone();
                                        drop(current_client_data);
                                        let mut room_data = room.data.lock().await;

                                        let room_current_client = room_data.clients.iter().find(|room_client| room_client.client.uid == current_client.uid).unwrap();
                                        if !room_current_client.owner {
                                            response_with_error(&current_client, ErrorKind::Forbidden);
                                        }

                                        let room_target_client = room_data.clients.iter_mut().find(|room_client| room_client.client.uid == client_uid);

                                        if let Some(room_target_client) = room_target_client {
                                            room_target_client.admin = admin;
                                            response_with_success(&current_client);
                                            broadcast_room_change(&room_data).await;
                                        } else {
                                            response_with_error(&current_client, ErrorKind::NoSuchClient);
                                        }
                                    }
                                }
                                IncomingMessage::QuitRoom => {
                                    if let Ok(mut current_client_data) = client_in_room(&current_client).await {
                                        handle_quit_room(&state, &current_client, current_client_data.deref_mut()).await;
                                        response_with_success(&current_client);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            response_with_error_msg(&current_client, ErrorKind::JsonError, format!("Invalid JSON: {}", e))
                        }
                    }
                }
            }

            handle_client_disconnect(&state, &current_client).await;

            Ok(())
        })
    })
}

async fn handle_client_disconnect(state: &Arc<WsAppState>, current_client: &Arc<Client>) {
    {
        let mut current_client_data = current_client.data.lock().await;

        if current_client_data.room.is_some() {
            handle_quit_room(state, current_client, current_client_data.deref_mut()).await;
        }
    }

    let mut clients = state.clients.lock().await;
    let index = clients
        .iter()
        .position(|x| Arc::ptr_eq(x, current_client))
        .unwrap();
    clients.remove(index);
}

// Room existence must be checked before calling
async fn handle_quit_room(state: &Arc<WsAppState>, current_client: &Arc<Client>, current_client_data: &mut ClientData) {
    let room = current_client_data.room.as_ref().unwrap().clone();
    current_client_data.room = None;

    let mut room_data = room.data.lock().await;
    room_data.remove_client(&current_client);

    if room_data.clients.is_empty() {
        state.rooms.lock().await.remove(&room.room_id);
    } else {
        broadcast_room_change(&room_data).await;
    }
}

async fn client_in_room<'a>(current_client: &'a Arc<Client>) -> Result<MutexGuard<'a, ClientData>, ()> {
    let current_client_data = current_client.data.lock().await;

    if let Some(room) = &current_client_data.room {
        Ok(current_client_data)
    } else {
        response_with_error(&current_client, ErrorKind::ClientNotInAnyRoom);
        Err(())
    }
}

async fn broadcast_room_change(room_data: &RoomData) {
    let payload = serde_json::to_string(&OutgoingMessage::RoomChanged { data: RoomDataDto::from(room_data).await }).unwrap();
    for client in room_data.clients.iter() {
        let _ = response_with_text(&client.client, payload.clone());
    }
}

async fn validate_client_name(current_client: &Client) -> bool {
    if current_client.data.lock().await.name.is_none() {
        response_with_error(current_client, ErrorKind::ClientNameNotSet);
        false
    } else {
        true
    }
}

fn response_with_text(current_client: &Client, payload: String) -> Result<(), SendError<Message>> {
    current_client.tx.send(ws::Message::Text(payload))
}

fn response_with_json(current_client: &Client, payload: OutgoingMessage) {
    let _ = response_with_text(current_client, serde_json::to_string(&payload).unwrap());
}

fn response_with_success(current_client: &Client) {
    response_with_json(current_client, OutgoingMessage::Success)
}

fn response_with_error(current_client: &Client, error_kind: ErrorKind) {
    response_with_json(current_client, OutgoingMessage::Error {
        kind: error_kind,
        msg: None
    })
}

fn response_with_error_msg(current_client: &Client, error_kind: ErrorKind, msg: String) {
    response_with_json(current_client, OutgoingMessage::Error {
        kind: error_kind,
        msg: Some(msg)
    })
}