#![cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
extern crate actix;
extern crate actix_web;
extern crate env_logger;

use std::sync::Arc;
use std::sync::Mutex;

use actix_web::{middleware, server, App, HttpRequest, HttpResponse};

/// Application state
struct AppState {
    counter: Arc<Mutex<usize>>,
}

/// simple handle
fn index(req: &HttpRequest<AppState>) -> HttpResponse {
    println!("{:?}", req);
    *(req.state().counter.lock().unwrap()) += 1;

    HttpResponse::Ok().body(format!("Num of requests: {}", req.state().counter.lock().unwrap()))
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();
    let sys = actix::System::new("ws-example");

    let counter = Arc::new(Mutex::new(0));
    //move is necessary to give closure below ownership of counter
    server::new(move || {
        App::with_state(AppState{counter: counter.clone()}) // <- create app with shared state
            // enable logger
            .middleware(middleware::Logger::default())
            // register simple handler, handle all methods
            .resource("/", |r| r.f(index))
    }).bind("127.0.0.1:8080")
        .unwrap()
        .start();

    println!("Started http server: 127.0.0.1:8080");
    let _ = sys.run();
}
