use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use deadpool_postgres::Pool;
use tokio::sync::RwLock;
use viz::{types::{Params, State}, Request, RequestExt, Result, Router, Server, ServiceMaker, StatusCode};
use crate::{bulk_search_tree::{BulkSearchTree, User}, postgres::init_user};

#[derive(Clone)]
struct HTTPState {
    pool: &'static Pool,
    tree: &'static BulkSearchTree,
    dids: &'static RwLock<HashMap<String, Arc<User>>>,
    http_key: &'static str,
}

async fn private_key_handler(mut req: Request) -> Result<StatusCode> {
    // Extract the key and HTTP state.
    let (State(state), Params(key)) = req.extract::<(State<HTTPState>, Params<String>)>().await?;

    // Check the authorization header.
    let auth = match req.headers().get("Authorization") {
        Some(auth) => auth,
        None => return Ok(StatusCode::BAD_REQUEST),
    };
    let auth = auth.to_str().unwrap();

    // Check the key in constant time.
    if !crypto::util::fixed_time_eq(auth.as_bytes(), state.http_key.as_bytes()) {
        return Ok(StatusCode::UNAUTHORIZED);
    }

    // Call the function to init a user from the pg file.
    init_user(state.pool, state.tree, state.dids, &key).await;

    // Return a 204.
    Ok(StatusCode::NO_CONTENT)
}

pub async fn init_http_server(
    pool: &'static Pool, tree: &'static BulkSearchTree, dids: &'static RwLock<HashMap<String, Arc<User>>>,
) {
    // Get the HTTP key.
    let http_key = Box::leak(Box::new(std::env::var("HTTP_KEY").unwrap()));

    // Get the host to serve on.
    let host = std::env::var("HOST").unwrap_or("0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or("6969".to_string());

    // Turn the port into a u16.
    let port = port.parse::<u16>().unwrap();

    // Create the HTTP server.
    let router = Router::new()
        .put("/:key", private_key_handler)
        .with(State::new(HTTPState { pool, tree, dids, http_key }));

    // Serve the router.
    let addr = format!("{host}:{port}").parse::<SocketAddr>().unwrap();
    if let Err(err) = Server::bind(&addr).serve(ServiceMaker::from(router)).await {
        panic!("Error binding to {}: {}", addr, err);
    }
}
