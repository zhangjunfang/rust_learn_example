extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate futures;

use actix_web::{
    client, middleware, server, App, AsyncResponder, Body, Error, HttpMessage,
    HttpRequest, HttpResponse,
};
use futures::{Future, Stream};

/// Stream client request response and then send body to a server response
fn index(_req: &HttpRequest) -> Box<Future<Item = HttpResponse, Error = Error>> {
    client::ClientRequest::get("http://127.0.0.1:8081/")
        .finish().unwrap()
        .send()
        .map_err(Error::from)          // <- convert SendRequestError to an Error
        .and_then(
            |resp| resp.body()         // <- this is MessageBody type, resolves to complete body
                .from_err()            // <- convert PayloadError to an Error
                .and_then(|body| {     // <- we got complete body, now send as server response
                    Ok(HttpResponse::Ok().body(body))
                }))
        .responder()
}

/// streaming client request to a streaming server response
fn streaming(_req: &HttpRequest) -> Box<Future<Item = HttpResponse, Error = Error>> {
    // send client request
    client::ClientRequest::get("https://www.rust-lang.org/en-US/")
        .finish().unwrap()
        .send()                         // <- connect to host and send request
        .map_err(Error::from)           // <- convert SendRequestError to an Error
        .and_then(|resp| {              // <- we received client response
            Ok(HttpResponse::Ok()
               // read one chunk from client response and send this chunk to a server response
               // .from_err() converts PayloadError to an Error
               .body(Body::Streaming(Box::new(resp.payload().from_err()))))
        })
        .responder()
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();
    let sys = actix::System::new("http-proxy");

    server::new(|| {
        App::new()
            .middleware(middleware::Logger::default())
            .resource("/streaming", |r| r.f(streaming))
            .resource("/", |r| r.f(index))
    }).workers(1)
        .bind("127.0.0.1:8080")
        .unwrap()
        .start();

    println!("Started http server: 127.0.0.1:8080");
    let _ = sys.run();
}
