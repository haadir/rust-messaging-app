use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, middleware::Logger, web};
use actix_ws::Message;
use futures::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex}, // synchronization primitives, they live in std::sync
};
use tokio::sync::mpsc;

type ClientId = u64;
type Tx = mpsc::UnboundedSender<String>;

#[derive(Default)]
struct AppState {
    // room_id -> number of connections
    rooms: HashMap<String, usize>,
    // room_id -> (client_id -> sender-to-that-client)
    clients: HashMap<String, HashMap<ClientId, Tx>>,
}

// Shared handle type
// ARC = Atomic Reference Counter
// stores data on the heap, lets you share ownership of the same value, even across threads
type SharedState = Arc<Mutex<AppState>>;

// accept shared state in websocket handler
async fn ws(
    state: web::Data<SharedState>,
    req: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;

    // For now, single room. Next step is /ws/{room_id}.
    let room_id = "default".to_string();
    let client_id: ClientId = rand::random();

    // Each client gets an outgoing mailbox (server -> this client)
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register client in shared state
    {
        let mut st = state.lock().unwrap();

        *st.rooms.entry(room_id.clone()).or_insert(0) += 1;

        st.clients
            .entry(room_id.clone())
            .or_default()
            .insert(client_id, tx);

        let count = st.rooms.get(&room_id).copied().unwrap_or(0);
        println!("CONNECT room='{room_id}' client={client_id} count={count}");
    }

    // Writer task: sends messages from rx -> websocket session
    actix_web::rt::spawn(async move {
        while let Some(payload) = rx.recv().await {
            if session.text(payload).await.is_err() {
                break;
            }
        }
        let _ = session.close(None).await;
    });

    // Reader task: reads client messages -> broadcast to room
    actix_web::rt::spawn({
        let state = state.clone();
        let room_id = room_id.clone();
        async move {
            while let Some(Ok(msg)) = msg_stream.next().await {
                match msg {
                    Message::Text(text) => {
                        println!("SERVER RECEIVED: {text}");

                        // 1) grab all senders in the room (clone them) under lock
                        let targets: Vec<Tx> = {
                            let st = state.lock().unwrap();
                            st.clients
                                .get(&room_id)
                                .map(|m| m.values().cloned().collect())
                                .unwrap_or_default()
                        };

                        // 2) broadcast outside lock
                        for t in targets {
                            let _ = t.send(text.to_string());
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(_)
                    | Message::Pong(_)
                    | Message::Binary(_)
                    | Message::Continuation(_)
                    | Message::Nop => {
                        // NOTE: Since `session` lives in the writer task, we can't pong here.
                        // For a "proper" ping/pong implementation, we can restructure next.
                    }
                }
            }

            // Cleanup on disconnect
            let mut st = state.lock().unwrap();

            // Remove client sender from room
            if let Some(room_clients) = st.clients.get_mut(&room_id) {
                room_clients.remove(&client_id);
                if room_clients.is_empty() {
                    st.clients.remove(&room_id);
                }
            }

            // Decrement room count
            if let Some(c) = st.rooms.get_mut(&room_id) {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    st.rooms.remove(&room_id);
                }
            }

            println!(
                "DISCONNECT room='{}' client={} count={}",
                room_id,
                client_id,
                st.rooms.get(&room_id).copied().unwrap_or(0)
            );
        }
    });

    Ok(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // initialize logger
    env_logger::init();

    // create shared application state, defined outside main, but create the value inside main
    let state: SharedState = Arc::new(Mutex::new(AppState::default()));

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default()) // logs each http request
            .app_data(web::Data::new(state.clone())) // clone increment of count
            .route("/ws", web::get().to(ws)) // registers a route
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;

    Ok(())
}
