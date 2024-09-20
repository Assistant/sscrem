use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
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

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
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
        }
    });

    client.join("squishywishyboo".to_owned()).unwrap();

    let router = Router::new()
        .route("/", get(root))
        .route("/raw", get(raw))
        .with_state(oscrems);

    let listener = tokio::net::TcpListener::bind(&"127.0.0.1:3333")
        .await
        .unwrap();

    axum::serve(listener, router).await.unwrap();

    join_handle.await.unwrap();
}

async fn root(screms: State<Arc<RwLock<u32>>>) -> Html<String> {
    let screms = screms.read().await;
    Html(format!(
        r#"<!DOCTYPE html><html lang="en"><head><script>setInterval(() => {{fetch(`${{location.protocol}}//${{location.host}}/raw`).then(res => {{if (res.ok) return res.text()}}).then(v => {{document.getElementsByTagName('body')[0].innerHTML = v}})}},100)</script></head><body>{}</body></html>"#,
        *screms
    ))
}

async fn raw(screms: State<Arc<RwLock<u32>>>) -> String {
    let screms = screms.read().await;
    (*screms).to_string()
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
