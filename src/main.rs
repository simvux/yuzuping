use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio;
use tokio::sync::Semaphore;

#[derive(Serialize, Deserialize, Debug)]
struct Room {
    port: u32,
    name: String,
    description: Option<String>,
    #[serde(rename = "preferredGameName")]
    game_name: String,
    address: String,
    players: Vec<Player>,

    #[serde(skip)]
    ping: Option<Duration>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Player {
    nickname: String,
    #[serde(rename = "gameName")]
    game: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    rooms: Vec<Room>,
}

#[tokio::main]
async fn main() {
    let url = std::env::var("YUZU_LOBBY_URL")
        .unwrap_or_else(|_| String::from("https://api.yuzu-emu.org/lobby"));

    let game_name = std::env::var("YUZU_GAME_NAME")
        .unwrap_or_else(|_| String::from("Super Smash Bros. Ultimate"));

    let resp = reqwest::get(url)
        .await
        .unwrap()
        .json::<Response>()
        .await
        .unwrap();

    let mut rooms = resp
        .rooms
        .into_iter()
        .filter(|room| room.game_name == game_name)
        .collect::<Vec<_>>();

    let semaphore = Arc::new(Semaphore::new(10));

    let total = rooms.len();
    let count = AtomicU64::new(0);

    let pings = rooms.iter_mut().map(|room| async {
        let _permit = semaphore.clone().acquire_owned().await.unwrap();

        let c = count.load(Ordering::Relaxed);
        println!("{}/{}", c, total);
        count.store(c + 1, Ordering::Relaxed);

        match ping(&room.address).await {
            Ok(output) => room.ping = output_to_duration(output),
            Err(err) => eprintln!("unable to ping: {err}"),
        }
    });

    join_all(pings).await;

    rooms.sort_by(|left, right| none_is_high(&left.ping).cmp(&none_is_high(&right.ping)));

    for room in rooms.iter().rev() {
        if let Some(ping) = room.ping {
            println!(
                "{} ({} playing)  {:?}",
                &room.name,
                room.players.len(),
                ping
            );
        }
    }

    println!(" - press enter to exit - ");

    std::io::stdin().read_line(&mut String::new()).unwrap();
}

fn none_is_high(dur: &Option<Duration>) -> Duration {
    dur.unwrap_or_else(|| Duration::from_secs(1000))
}

// we spawn a subshell instead of using ICMP directly because then we don't require
// sudo/administrator or setcap.

#[cfg(windows)]
fn ping(address: &str) -> impl Future<Output = Result<std::process::Output, std::io::Error>> {
    tokio::process::Command::new("ping")
        .arg("-n")
        .arg("3")
        .arg("-w")
        .arg("500")
        .arg(address)
        .output()
}
#[cfg(not(windows))]
fn ping(address: &str) -> impl Future<Output = Result<std::process::Output, std::io::Error>> {
    tokio::process::Command::new("ping")
        .arg("-c")
        .arg("3")
        .arg("-W")
        .arg("0.5")
        .arg(address)
        .output()
}

fn output_to_duration(output: std::process::Output) -> Option<Duration> {
    output
        .stdout
        .windows(12)
        .filter_map(|window| {
            window.starts_with(b"time=").then(|| {
                let end = window[5..]
                    .iter()
                    .position(|b| !b.is_ascii_digit())
                    .unwrap_or(window.len());

                let ping = std::str::from_utf8(&window[5..5 + end])
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();

                ping
            })
        })
        .min()
        .map(Duration::from_millis)
}
