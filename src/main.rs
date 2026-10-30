//! Speed updating for a room of people.
//!
//! Everybody says where they stand on a set of statements, then talks to
//! whoever disagrees with them most, a few minutes at a time, and says
//! where they stand afterwards.
//!
//! Three kinds of link, and the key in each is the only authority there
//! is: `/h/<key>` runs the room, `/j/<key>` joins it, and `/p/<key>`
//! belongs to one player.

mod game;
mod statements;
mod view;

use std::{
    io::Write,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use rooms::{Room, Rooms, http, now_ms, qr};
use serde::Deserialize;
use tokio::sync::Mutex;

use game::{Game, Mode, Scale};

const INDEX: &str = include_str!("../static/index.html");
const ICON: &str = include_str!("../static/icon.svg");
const APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.js"));
const APP_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.css"));

const MAX_ROOMS: usize = 2000;
const SWEEP: Duration = Duration::from_secs(5 * 60);
/// Suggested statements are kept until somebody reads them; past this the
/// file takes no more, so nobody can fill the disk with it.
const MAX_SUGGESTIONS_BYTES: u64 = 1 << 20;

#[derive(Clone)]
struct App {
    rooms: Arc<Rooms<Game>>,
    suggestions: Arc<Mutex<PathBuf>>,
}

fn refused(message: &str) -> Response {
    http::error(StatusCode::CONFLICT, message)
}

/// Applies a facilitator's action and answers with their whole view.
async fn act(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Game, u64) -> Result<(), &'static str>,
) -> Response {
    let done = app
        .rooms
        .with(key, |game, now| {
            game.tick(now);
            action(game, now).map(|()| http::envelope(now, &view::host(game)))
        })
        .await;
    match done {
        None => http::gone(),
        Some(Err(message)) => refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

/// The same for a player: finds their room by their key, then acts.
async fn act_as(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Game, u64) -> Result<(), &'static str>,
) -> Response {
    let Some(room) = app
        .rooms
        .find(|room, game| game.player(key).map(|_| room.to_string()))
        .await
    else {
        return http::gone();
    };
    let done = app
        .rooms
        .with(&room, |game, now| {
            game.tick(now);
            action(game, now)?;
            view::player(game, key)
                .map(|v| http::envelope(now, &v))
                .ok_or("You are not in this room.")
        })
        .await;
    match done {
        None => http::gone(),
        Some(Err(message)) => refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

#[derive(Deserialize)]
struct Setup {
    mode: Mode,
    #[serde(default)]
    scale: Scale,
    topics: Vec<String>,
    #[serde(default)]
    custom: String,
    side: u8,
}

#[derive(Deserialize)]
struct Minutes {
    minutes: u32,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Answer {
    id: u32,
    value: Option<i16>,
}

#[derive(Deserialize)]
struct Mark {
    id: u32,
    marked: bool,
}

#[derive(Deserialize)]
struct Skip {
    id: u32,
}

#[derive(Deserialize)]
struct Suggestion {
    text: String,
    #[serde(default)]
    chance: bool,
}

async fn open_room(State(app): State<App>) -> Response {
    let now = now_ms();
    let Some(host) = app.rooms.create(|_| Game::new(now)).await else {
        return http::error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Too many rooms are open at once. Try again in a few minutes.",
        );
    };
    let join = app.rooms.read(&host, |game, _| game.join.clone()).await;
    Json(serde_json::json!({ "host": host, "join": join })).into_response()
}

async fn host_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |_, _| Ok(())).await
}

async fn host_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::one(app.rooms, key, |game| {
        serde_json::to_string(&view::host(game)).ok()
    })
}

async fn setup(State(app): State<App>, Path(key): Path<String>, Json(s): Json<Setup>) -> Response {
    act(&app, &key, |game, _| {
        game.setup(s.mode, s.scale, &s.topics, &s.custom, s.side)
    })
    .await
}

async fn minutes(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(m): Json<Minutes>,
) -> Response {
    act(&app, &key, |game, _| game.set_minutes(m.minutes)).await
}

async fn round(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, now| game.start_round(now)).await
}

async fn stop(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.stop()).await
}

async fn extend(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, now| game.extend(now)).await
}

async fn finish(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.finish()).await
}

async fn reopen(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.reopen()).await
}

async fn room_with_join(app: &App, join: &str) -> Option<String> {
    let join = join.to_string();
    app.rooms
        .find(move |key, game| (game.join == join).then(|| key.to_string()))
        .await
}

async fn join(
    State(app): State<App>,
    Path(join): Path<String>,
    Json(named): Json<Named>,
) -> Response {
    let Some(key) = room_with_join(&app, &join).await else {
        return http::gone();
    };
    match app
        .rooms
        .with(&key, |game, now| game.join(&named.name, now))
        .await
    {
        None => http::gone(),
        Some(Err(message)) => refused(message),
        Some(Ok(player)) => Json(serde_json::json!({ "player": player })).into_response(),
    }
}

async fn whose(State(app): State<App>, Path(join): Path<String>) -> Response {
    let Some(key) = room_with_join(&app, &join).await else {
        return http::gone();
    };
    match app.rooms.read(&key, |game, _| game.players.len()).await {
        Some(players) => Json(serde_json::json!({ "players": players })).into_response(),
        None => http::gone(),
    }
}

async fn player_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    act_as(&app, &key, |_, _| Ok(())).await
}

async fn player_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::stream(app.rooms, move |held, now| {
        let game = held.values_mut().find(|game| game.player(&key).is_some())?;
        game.last_seen = now;
        // The clock ends a round even when the facilitator's screen is
        // closed. Saved by the next request, like everything a watcher sees.
        game.tick(now);
        serde_json::to_string(&view::player(game, &key)?).ok()
    })
}

async fn answer(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(a): Json<Answer>,
) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| {
        game.answer(&player, a.id, a.value)
    })
    .await
}

async fn mark(State(app): State<App>, Path(key): Path<String>, Json(m): Json<Mark>) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| {
        game.mark(&player, m.id, m.marked)
    })
    .await
}

async fn skip(State(app): State<App>, Path(key): Path<String>, Json(s): Json<Skip>) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| game.skip(&player, s.id)).await
}

/// Keeps a statement somebody would like added, one JSON line each, for
/// whoever runs the server to read. Nothing reads it back over the web.
async fn suggest(State(app): State<App>, Json(s): Json<Suggestion>) -> Response {
    let text = game::tidy(&s.text, game::MAX_STATEMENT);
    if text.chars().count() < 8 {
        return refused("Write the statement out, so it can be added as it is.");
    }
    let file = app.suggestions.lock().await;
    match keep(&file, &text, s.chance, now_ms()) {
        Ok(()) => Json(serde_json::json!({ "kept": true })).into_response(),
        Err(message) => http::error(StatusCode::SERVICE_UNAVAILABLE, message),
    }
}

fn keep(path: &FsPath, text: &str, chance: bool, now: u64) -> Result<(), &'static str> {
    let full = "There are a lot of suggestions waiting already. Try again another day.";
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_SUGGESTIONS_BYTES) {
        return Err(full);
    }
    let line = serde_json::json!({ "at": now, "text": text, "chance": chance }).to_string();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| writeln!(f, "{line}"))
        .map_err(|_| "The suggestion could not be saved. Try again later.")
}

async fn qr_code(body: Bytes) -> Response {
    match std::str::from_utf8(&body) {
        Ok(link) => qr::response(link),
        Err(_) => http::error(StatusCode::BAD_REQUEST, "That is not a link."),
    }
}

async fn time() -> Response {
    http::json(format!("{{\"now\":{}}}", now_ms()))
}

async fn page() -> Html<&'static str> {
    Html(INDEX)
}

fn asset(kind: &'static str, cache: &'static str, body: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, cache)],
        body,
    )
        .into_response()
}

async fn script() -> Response {
    asset("text/javascript; charset=utf-8", "no-cache", APP_JS)
}

async fn stylesheet() -> Response {
    asset("text/css; charset=utf-8", "no-cache", APP_CSS)
}

async fn icon() -> Response {
    asset("image/svg+xml", "max-age=3600", ICON)
}

fn router(app: App) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/h/{key}", get(page))
        .route("/j/{key}", get(page))
        .route("/p/{key}", get(page))
        .route("/icon.svg", get(icon))
        .route("/app.js", get(script))
        .route("/app.css", get(stylesheet))
        .route("/api/time", get(time))
        .route("/api/qr", post(qr_code))
        .route("/api/suggest", post(suggest))
        .route("/api/rooms", post(open_room))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/setup", put(setup))
        .route("/api/h/{key}/minutes", put(minutes))
        .route("/api/h/{key}/round", post(round))
        .route("/api/h/{key}/stop", post(stop))
        .route("/api/h/{key}/extend", post(extend))
        .route("/api/h/{key}/finish", post(finish))
        .route("/api/h/{key}/reopen", post(reopen))
        .route("/api/j/{key}", get(whose).post(join))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/answer", put(answer))
        .route("/api/p/{key}/mark", put(mark))
        .route("/api/p/{key}/skip", put(skip))
        .with_state(app)
}

#[tokio::main]
async fn main() {
    let addr = std::env::var("UPDATING_ADDR").unwrap_or_else(|_| "127.0.0.1:8102".into());
    let state = std::env::var("UPDATING_STATE").unwrap_or_else(|_| "rooms.json".into());
    let suggestions =
        std::env::var("UPDATING_SUGGESTIONS").unwrap_or_else(|_| "suggestions.jsonl".into());

    let rooms = Rooms::load(PathBuf::from(state), MAX_ROOMS);
    tokio::spawn(rooms.clone().sweep(SWEEP));
    let app = App {
        rooms,
        suggestions: Arc::new(Mutex::new(PathBuf::from(suggestions))),
    };

    let listener = rooms::listen(&addr).await;
    println!("{}", rooms::where_it_listens(&listener));
    axum::serve(listener, router(app))
        .await
        .expect("the server stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    struct Test {
        app: Router,
        files: [PathBuf; 2],
    }

    impl Drop for Test {
        fn drop(&mut self) {
            for f in &self.files {
                let _ = std::fs::remove_file(f);
            }
        }
    }

    fn test(name: &str) -> Test {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let state = dir.join(format!("updating-test-{name}-{id}.json"));
        let ideas = dir.join(format!("updating-test-{name}-{id}.jsonl"));
        let _ = std::fs::remove_file(&state);
        let _ = std::fs::remove_file(&ideas);
        let app = router(App {
            rooms: Rooms::load(&state, 4),
            suggestions: Arc::new(Mutex::new(ideas.clone())),
        });
        Test {
            app,
            files: [state, ideas],
        }
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn ok(app: &Router, method: &str, uri: &str, body: Option<Value>) -> Value {
        let (status, body) = call(app, method, uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {body}");
        body
    }

    /// An open room with two people in it.
    async fn ready(app: &Router) -> (String, String, String) {
        let keys = ok(app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        let join = keys["join"].as_str().unwrap().to_string();
        ok(
            app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "open", "topics": ["mind"], "side": 4})),
        )
        .await;
        let mut players = Vec::new();
        for name in ["Ada", "Bo"] {
            let body = ok(
                app,
                "POST",
                &format!("/api/j/{join}"),
                Some(json!({"name": name})),
            )
            .await;
            players.push(body["player"].as_str().unwrap().to_string());
        }
        (host, players[0].clone(), players[1].clone())
    }

    #[tokio::test]
    async fn two_people_who_disagree_are_paired_and_see_each_other() {
        let t = test("pair");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 90})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 0, "value": 10})),
        )
        .await;

        let view = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(view["opposites"][0]["name"], "Bo", "Bo is the one to find");
        assert_eq!(view["opposites"][0]["gap"], 160);

        let host_view =
            ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await["view"].clone();
        assert_eq!(host_view["phase"], "talking");
        assert_eq!(host_view["round"]["groups"][0]["gap"], 160);

        let view = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(view["group"]["partners"][0]["name"], "Bo");
        assert_eq!(view["group"]["partners"][0]["answer"], 10);
        assert_eq!(view["group"]["before"], 90);

        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 40})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["group"]["after"], 40);
        assert_eq!(view["updated"], 1);
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(host_view["updates"], 1);
        assert_eq!(host_view["round"]["groups"][0]["people"][0]["after"], 40);
    }

    #[tokio::test]
    async fn a_player_is_not_sent_other_peoples_answers_beyond_their_partners() {
        let t = test("private");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 3, "value": 17})),
        )
        .await;
        let view = ok(&t.app, "GET", &format!("/api/p/{bo}"), None).await;
        assert!(
            view["view"]["opposites"].as_array().unwrap().is_empty(),
            "nothing in common yet"
        );
        let (status, _) = call(&t.app, "GET", &format!("/api/h/{ada}"), None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a player key does not run the room"
        );
        let (status, _) = call(&t.app, "GET", &format!("/api/p/{host}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn in_the_free_mode_who_answered_is_shown_only_after_answering() {
        let t = test("free");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "free", "topics": ["mind"], "side": 4})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 2, "value": 23})),
        )
        .await;

        let before = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(
            before["squares"][2]["count"], 1,
            "Ada sees that somebody answered"
        );
        assert!(
            before["squares"][2]["others"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !before.to_string().contains("\"answer\":23"),
            "but not what or who"
        );

        let after = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 2, "value": 60})),
        )
        .await["view"]
            .clone();
        assert_eq!(after["squares"][2]["others"][0]["name"], "Bo");
        assert_eq!(after["squares"][2]["others"][0]["answer"], 23);
        assert_eq!(
            after["opposites"][0]["name"], "Bo",
            "and Bo is the one to go and find"
        );
    }

    #[tokio::test]
    async fn suggestions_are_kept_on_disk_and_never_served() {
        let t = test("suggest");
        ok(
            &t.app,
            "POST",
            "/api/suggest",
            Some(json!({"text": "  Octopuses   are conscious. ", "chance": false})),
        )
        .await;
        let (status, _) = call(&t.app, "POST", "/api/suggest", Some(json!({"text": "hi"}))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let kept = std::fs::read_to_string(&t.files[1]).unwrap();
        assert_eq!(kept.lines().count(), 1);
        assert!(kept.contains("\"text\":\"Octopuses are conscious.\""));
        let (status, _) = call(&t.app, "GET", "/api/suggest", None).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn the_setup_is_refused_once_people_have_answered() {
        let t = test("lock");
        let (host, ada, _) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 0})),
        )
        .await;
        let (status, body) = call(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "bingo", "topics": ["ea"], "side": 3})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("started answering")
        );
    }

    #[tokio::test]
    async fn a_room_survives_the_server_restarting() {
        let t = test("restart");
        let (host, ada, _) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 1, "value": 20})),
        )
        .await;
        let again = router(App {
            rooms: Rooms::load(&t.files[0], 4),
            suggestions: Arc::new(Mutex::new(t.files[1].clone())),
        });
        let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["answers"], 1);
    }

    #[tokio::test]
    async fn the_page_is_served_at_every_link() {
        let t = test("pages");
        for uri in [
            "/",
            "/h/abc",
            "/j/abc",
            "/p/abc",
            "/app.js",
            "/app.css",
            "/icon.svg",
        ] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = t.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }
}
