use axum::extract::ws::Message::Text;
use axum::extract::{ws::WebSocket, State, WebSocketUpgrade};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use tokio::sync::watch::{self, Receiver};
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::ServerMessage::Privmsg;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, SecureTCPTransport};

#[tokio::main]
pub async fn main() {
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    let (tx, rx) = watch::channel(0u32);
    let mut screms = 0u32;

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            let last_screms = screms;
            if let Privmsg(message) = message {
                match message.message_text.as_str() {
                    "!screm" => screms += 1,
                    "!noscrem" if allow_edit(&message.badges) && screms > 0 => screms -= 1,
                    "!reset" if allow_reset(&message.badges) => screms = 0,
                    n if n.starts_with("!scremset ") && allow_reset(&message.badges) => {
                        if let Ok(s) = n[10..].parse() {
                            screms = s;
                        }
                    }
                    _ => {}
                }
            }
            if screms != last_screms {
                tx.send(screms).unwrap();
            }
        }
    });

    client.join("squishywishyboo".to_owned()).unwrap();

    let router = Router::new()
        .route("/", get(root))
        .route("/ws", get(handler))
        .with_state(rx);

    let listener = tokio::net::TcpListener::bind(&"127.0.0.1:3333")
        .await
        .unwrap();

    axum::serve(listener, router).await.unwrap();

    join_handle.await.unwrap();
}

async fn handler(ws: WebSocketUpgrade, state: State<Receiver<u32>>) -> Response {
    ws.on_upgrade(|s| handle_socket(s, state))
}

async fn handle_socket(mut socket: WebSocket, State(mut rx): State<Receiver<u32>>) {
    loop {
        let Ok(()) = rx.changed().await else { return };
        let screm = *rx.borrow_and_update();
        let Ok(()) = socket.send(Text(screm.to_string())).await else {
            return;
        };
    }
}

async fn root(State(screms): State<Receiver<u32>>) -> Html<String> {
    let screms = *screms.borrow();
    Html(format!(
        r#"<!DOCTYPE html><html lang="en"><head><script>const socket = new WebSocket(`${{location.protocol == "https:" ? "wss:" : "ws:"}}//${{location.host}}/ws`);socket.addEventListener("message", event => document.getElementsByTagName('body')[0].innerHTML = event.data)</script></head><body>{screms}</body></html>"#
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
