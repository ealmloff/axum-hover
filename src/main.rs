use std::convert::Infallible;
use std::fmt::Write;
use std::sync::{Arc, RwLock};

use axum::extract::Path;
use axum::http::StatusCode;
use axum::{
    body::Body,
    http::{header, HeaderValue},
    routing::get,
    Router,
};
use futures_channel::mpsc::{channel, Receiver, Sender};
use futures_util::StreamExt as _;

struct Renderer {
    last_mount: usize,
    channel: Sender<String>,
    root: Mount,
}

impl Renderer {
    fn new(channel: Sender<String>) -> Self {
        let mut myself = Self {
            last_mount: 0,
            channel,
            root: Mount { id: 0 },
        };
        _ = myself
            .channel
            .start_send(r#"<div><template shadowrootmode="open">"#.to_string());
        let mount = myself.start_slot();
        myself.end_slot();
        _ = myself.channel.start_send("</template></div>".to_string());
        myself.root = mount;
        myself
    }

    fn render(&mut self, html: String) {
        let root = Mount { id: self.root.id };
        let segments = r#"<div style="display: none"></div>"#.to_string();
        let root = self.replace(root, segments.clone());
        let close = "</div>".repeat(2);
        _ = self.channel.start_send(close);
        self.root = self.replace(root, html);
    }

    fn mount(&mut self) -> Mount {
        let mount = self.last_mount;
        self.last_mount += 1;
        Mount { id: mount }
    }

    fn start_slot(&mut self) -> Mount {
        let mount = self.mount();
        let id = mount.id;
        _ = self
            .channel
            .start_send(format!(r#"<slot name="dioxus-{id}">"#));
        mount
    }

    fn end_slot(&mut self) {
        _ = self.channel.start_send("</slot>".to_string());
    }

    fn replace(&mut self, mount: Mount, html: String) -> Mount {
        let mounted_id = mount.id;
        _ = self.channel.start_send(format!(
            r#"<div slot="dioxus-{mounted_id}"><template shadowrootmode="open">"#
        ));

        let mount = self.start_slot();
        _ = self.channel.start_send(html);
        self.end_slot();

        _ = self.channel.start_send("</template>".to_string());
        mount
    }
}

struct Mount {
    id: usize,
}

struct State {
    grid: [[usize; 10]; 10],
    renderer: Option<Renderer>,
    uuid: usize,
}

impl State {
    fn new() -> Self {
        Self {
            grid: [[0; 10]; 10],
            renderer: None,
            uuid: 0,
        }
    }

    fn html(&mut self) -> String {
        let mut html = String::new();
        write!(html, r#"<div style="display: grid; grid-template-columns: repeat(10, 1fr); grid-template-rows: repeat(10, 1fr); width: 400px; height: 400px;">"#).unwrap();
        let uuid = self.uuid;
        for i in 0..10 {
            for j in 0..10 {
                let value = self.grid[i][j];
                write!(html, r#"<div id="grid-{i}-{j}-{uuid}" style="width: 100%; height: 100%; background-color: rgb(0%, {value}%, 0%);">
                <style>
                #grid-{i}-{j}-{uuid}:hover {{
                    background-image: url("/hover/{i}/{j}/{uuid}");
                }}
                </style>
                </div>"#).unwrap();
            }
        }
        self.uuid += 1;
        write!(html, r#"</div>"#).unwrap();
        html
    }

    fn update(&mut self, x: usize, y: usize) {
        self.grid[x][y] += 5;
        let html = self.html();
        if let Some(renderer) = &mut self.renderer {
            renderer.render(html);
        }
    }

    fn create_renderer(&mut self) -> Receiver<String> {
        let start_html = r#"<!DOCTYPE html>
<head>
    <title>Hello streaming</title>
</head>
<body>"#;
        let (mut tx, rx) = channel(1000);
        _ = tx.start_send(start_html.to_string());
        let mut renderer = Renderer::new(tx);
        let html = self.html();
        renderer.render(html);
        self.renderer = Some(renderer);

        rx
    }

    fn reset(&mut self) {
        self.grid = [[0; 10]; 10];
    }
}

#[tokio::main]
async fn main() {
    let state = Arc::new(RwLock::new(State::new()));

    // build our application with a single route
    let app = Router::new()
        .route(
            "/",
            get({
                let state = state.clone();
                || async move {
                    let mut state = state.write().unwrap();
                    state.reset();
                    let rx = state.create_renderer();
                    (
                        [(
                            header::CONTENT_TYPE,
                            HeaderValue::from_static(mime::TEXT_HTML_UTF_8.as_ref()),
                        )],
                        Body::from_stream(rx.map(Ok::<_, Infallible>)),
                    )
                }
            }),
        )
        .route(
            "/hover/:x/:y/:uuid",
            get({
                let state = state.clone();
                |Path((x, y, _uuid)): Path<(usize, usize, usize)>| async move {
                    let mut state = state.write().unwrap();
                    state.update(x, y);
                    StatusCode::NOT_FOUND
                }
            }),
        );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
