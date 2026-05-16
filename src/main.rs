use axum::extract::ws::{Message::Ping, Message::Text, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use tokio::sync::watch::{self, Receiver};
use tokio::time::{interval, Duration};
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::ServerMessage::Privmsg;
use twitch_irc::{ClientConfig, SecureTCPTransport, TwitchIRCClient};

#[tokio::main]
pub async fn main() {
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    let mut screms = get_state().unwrap_or_default();
    let (tx, rx) = watch::channel(screms);

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            if let Privmsg(message) = message {
                match message.message_text.as_str() {
                    "!screm" => screms += 1,
                    "!noscrem" if can_edit(&message.badges) && screms > 0 => screms -= 1,
                    "!reset" if can_reset(&message.badges) => screms = 0,
                    n if n.starts_with("!scremset ") && can_reset(&message.badges) => {
                        if let Ok(s) = n[10..].parse() {
                            screms = s;
                        }
                    }
                    _ => continue,
                }
                tx.send(screms).unwrap();
                save_state(screms);
            }
        }
    });

    client.join("squishywishyboo".to_owned()).unwrap();

    let router = Router::new()
        .route("/", get(root))
        .route("/ws", get(handler))
        .with_state(rx);

    let port = env::var("PORT").unwrap_or("8000".to_string());

    let listener = tokio::net::TcpListener::bind(&format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    axum::serve(listener, router).await.unwrap();

    join_handle.await.unwrap();
}

async fn handler(ws: WebSocketUpgrade, state: State<Receiver<u32>>) -> Response {
    ws.on_upgrade(|s| handle_socket(s, state))
}

async fn handle_socket(mut socket: WebSocket, State(mut rx): State<Receiver<u32>>) {
    let mut heartbeat = interval(Duration::from_secs(30));
    let screm = *rx.borrow_and_update();
    let Ok(()) = socket.send(Text(screm.to_string().into())).await else {
        return;
    };

    loop {
        tokio::select! {
            biased;
            _ = heartbeat.tick() => {
                let Ok(()) = socket.send(Ping(vec![].into())).await else { return };
            },
            Ok(()) = rx.changed() => {
                let screm = *rx.borrow_and_update();
                let Ok(()) = socket.send(Text(screm.to_string().into())).await else { return };
            },
            else => return,
        }
    }
}

async fn root() -> Html<String> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<script>
const url = `${location.protocol == "https:" ? "wss:" : "ws:"}//${location.host}/ws`;
const start = () => {
  let ws = new WebSocket(url);
  ws.onmessage = (m) => document.getElementsByTagName('body')[0].innerHTML = m.data;
  ws.onclose = () => setTimeout(start, 100);
};
start();
</script>
</head>
<body></body>
</html>"#
            .to_string(),
    )
}

fn can_reset(badges: &[twitch_irc::message::Badge]) -> bool {
    badges
        .iter()
        .any(|badge| badge.name == "broadcaster" || badge.name == "moderator")
}

fn can_edit(badges: &[twitch_irc::message::Badge]) -> bool {
    can_reset(badges)
        || badges
            .iter()
            .any(|badge| badge.name == "vip" || badge.name == "subscriber")
}

fn get_state() -> Option<u32> {
    let path = dirs::state_dir()?.join("screm/counter");
    fs::read_to_string(path).ok()?.parse().ok()
}

fn save_state(state: u32) -> Option<()> {
    let path = dirs::state_dir()?.join("screm/counter");
    let mut file = File::create(&path).ok()?;
    file.write_all(state.to_string().as_bytes()).ok()?;
    Some(())
}
