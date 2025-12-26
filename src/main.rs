use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, middleware::Logger, web};
use actix_ws::Message;
use futures::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex}, // synchronization primitives, they live in std::sync
};
use tokio::sync::mpsc;

#[derive(Default)]
struct AppState {
    // room_id will be number of connections in certain room
    rooms: HashMap<String, usize>,
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

    // /ws/{room_id}
    let room_id = "default".to_string();

    // clone state + room_id for spawned task
    let state = state.clone();

    actix_web::rt::spawn(async move {
        // On connect, update shared state
        {
            let mut st = state.lock().unwrap();
            *st.rooms.entry(room_id.clone()).or_insert(0) += 1;
        }

        // Normal message loop
        // while ws connection alive, keep reading messages from client
        while let Some(Ok(msg)) = msg_stream.next().await {
            match msg {
                // Ping "Are you still here?"
                // Pong "Yes, I'm here"
                Message::Ping(bytes) => {
                    if session.pong(&bytes).await.is_err() {
                        break;
                    }
                }
                // A UTF-8 string sent by the client
                Message::Text(text) => {
                    let count = {
                        let st = state.lock().unwrap();
                        st.rooms.get(&room_id).copied().unwrap_or(0)
                    };
                    println!("Got text: ")
                }
                _ => break,
            }
        }

        // disconnect, decrement and cleanup
        {
            let mut st = state.lock().unwrap();
            if let Some(c) = st.rooms.get_mut(&room_id) {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    st.rooms.remove(&room_id);
                }
            }
            println!(
                "DISCONNECT room='{}' count={}",
                room_id,
                st.rooms.get(&room_id).copied().unwrap_or(0)
            );
        }

        let _ = session.close(None).await;
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
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}
