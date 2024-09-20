use axum::extract::{ws::WebSocket, State, WebSocketUpgrade};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use spmc::Receiver;
use std::sync::Arc;
use tokio::sync::RwLock;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::ServerMessage::Privmsg;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, SecureTCPTransport};

#[tokio::main]
pub async fn main() {
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    let oscrems = Arc::new(RwLock::new(0u32));
    let screms = Arc::clone(&oscrems);
    let (mut tx, rx) = spmc::channel();

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            let last_screms = {
                let screms = screms.read().await;
                *screms
            };
            if let Privmsg(message) = message {
                match message.message_text.as_str() {
                    "!screm" => {
                        let mut screms = screms.write().await;
                        *screms += 1;
                    }
                    "!noscrem" if allow_edit(&message.badges) => {
                        let mut screms = screms.write().await;
                        if *screms > 0 {
                            *screms -= 1;
                        }
                    }
                    "!reset" if allow_reset(&message.badges) => {
                        let mut screms = screms.write().await;
                        *screms = 0;
                    }
                    n if n.starts_with("!scremset ") && allow_reset(&message.badges) => {
                        if let Ok(s) = n[10..].parse() {
                            let mut screms = screms.write().await;
                            *screms = s;
                        }
                    }
                    _ => {}
                }
            }
            let screms = screms.read().await;
            if *screms != last_screms {
                tx.send(*screms).unwrap()
            }
        }
    });

    client.join("squishywishyboo".to_owned()).unwrap();

    let router = Router::new()
        .route("/", get(root))
        .route("/ws", get(handler))
        .with_state((oscrems, rx));

    let listener = tokio::net::TcpListener::bind(&"127.0.0.1:3333")
        .await
        .unwrap();

    axum::serve(listener, router).await.unwrap();

    join_handle.await.unwrap();
}

async fn handler(
    ws: WebSocketUpgrade,
    state: State<(Arc<RwLock<u32>>, Receiver<u32>)>,
) -> Response {
    ws.on_upgrade(|s| handle_socket(s, state))
}
async fn handle_socket(
    mut socket: WebSocket,
    State((_, rx)): State<(Arc<RwLock<u32>>, Receiver<u32>)>,
) {
    while let Ok(screm) = rx.recv() {
        if socket
            .send(axum::extract::ws::Message::Text(screm.to_string()))
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn root(State((screms, _)): State<(Arc<RwLock<u32>>, Receiver<u32>)>) -> Html<String> {
    let screms = screms.read().await;
    Html(format!(
        r#"<!DOCTYPE html><html lang="en"><head><script>const socket = new WebSocket(`${{location.protocol == "https:" ? "wss:" : "ws:"}}//${{location.host}}/ws`);socket.addEventListener("message", event => {{document.getElementsByTagName('body')[0].innerHTML = event.data;}})</script></head><body>{}</body></html>"#,
        *screms
    ))
}

fn allow_reset(badges: &[twitch_irc::message::Badge]) -> bool {
    badges
        .iter()
        .any(|badge| badge.name == "broadcaster" || badge.name == "moderator")
}

fn allow_edit(badges: &[twitch_irc::message::Badge]) -> bool {
    allow_reset(badges)
        || badges
            .iter()
            .any(|badge| badge.name == "vip" || badge.name == "subscriber")
}
