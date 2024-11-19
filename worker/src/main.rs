mod bulk_search_tree;
mod http;
mod postgres;

use bulk_search_tree::{BulkSearchTree, User};
use deadpool_postgres::Pool;
use ed25519_dalek::ed25519::signature::SignerMut;
use futures::StreamExt as _;
use http::init_http_server;
use postgres::{delete_user, init_data, init_postgres};
use rsky_lexicon::{app::bsky::{feed::Post, richtext::Features}, com::atproto::sync::SubscribeRepos};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;
use std::{collections::{HashMap, HashSet}, io::Cursor, net::IpAddr, sync::{atomic::Ordering, Arc}, time::Duration};
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug, Deserialize)]
#[serde(tag = "$type")]
enum Lexicon {
    #[serde(rename(deserialize = "app.bsky.feed.post"))]
    AppBskyFeedPost(Post),
}

// Evicts a user if they are broken.
async fn evict_user(user: Arc<User>, tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>, pg_pool: &Pool) {
    // Remove the user from the trees.
    if let Some(did) = &user.did {
        dids.write().await.remove(did);
    }
    for phrase in &user.phrases {
        // This can be improved, but it is so rare that its not a big deal.
        tree.remove_item(phrase, user.clone()).await;
    }

    // Remove the user from Postgres.
    let reencoded_key = hex::encode(user.private_key.clone());
    delete_user(pg_pool, &reencoded_key).await;
}

// Handle if the server connection failed.
async fn server_conn_failed(user: Arc<User>, tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>, pg_pool: &Pool) {
    // Parse the URL.
    let url = match url::Url::parse(&user.endpoint) {
        Err(error) => {
            // WTF!
            eprintln!("Error parsing the user endpoint: {error:?}");
            evict_user(user, tree, dids, pg_pool).await;
            return;
        }
        Ok(url) => url,
    };

    // Check if the hostname is an IP address.
    let hostname = url.host_str().unwrap();
    if hostname.parse::<IpAddr>().is_ok() {
        // Don't do the DNS lookup.
        return;
    }

    // Check if there is either an A or AAAA record for the hostname.
    let mut lookup = match tokio::net::lookup_host(hostname).await {
        Ok(lookup) => lookup,
        Err(error) => {
            eprintln!("Error looking up the hostname: {error:?}");
            evict_user(user, tree, dids, pg_pool).await;
            return;
        }
    };

    // If there's nothing in the lookup, evict the user.
    if lookup.next().is_none() {
        evict_user(user, tree, dids, pg_pool).await;
    }
}

// Inform the user about the post.
async fn inform_user(
    user: Arc<User>, json: Arc<String>, ts_seconds: i64, http_client: reqwest::Client,
    tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>, pg_pool: &Pool,
) {
    // Perform a ED25519 signature of the json including the timestamp in seconds.
    let slice: &[u8; 32] = user.private_key.as_slice().try_into().unwrap();
    let mut signer = ed25519_dalek::SigningKey::from_bytes(slice);
    let ts_seconds_str = ts_seconds.to_string();
    let mut new_msg_body = format!("{ts_seconds_str}{json}");
    let signature = hex::encode(
        signer.sign(new_msg_body.as_bytes()).to_vec()
    );

    // Get after ts_seconds_str from new_msg_body and own it.
    let after_ts_seconds_str = new_msg_body.split_off(ts_seconds_str.len());

    // Send the message to the user.
    match 
        http_client.post(&user.endpoint).body(after_ts_seconds_str)
            .header("Content-Type", "application/json")
            .header("X-Signature-Ed25519", signature)
            .header("X-Signature-Timestamp", ts_seconds_str)
            .send().await
    {
        Err(_) => server_conn_failed(user, tree, dids, pg_pool).await,
        Ok(resp) => {
            if resp.status().is_success() {
                // Make sure the user downtime is reset.
                user.user_downtime_started.store(0, Ordering::Relaxed);
            } else {
                // If it is a 429 or 403, evict the user.
                let status_number = resp.status().as_u16();
                if status_number == 429 || status_number == 403 {
                    evict_user(user, tree, dids, pg_pool).await;
                    return;
                }

                // If not, figure out how long they have been down.
                let dt_start = user.user_downtime_started.load(Ordering::Relaxed);
                if dt_start == 0 {
                    // Mark this user as down and return.
                    user.user_downtime_started.store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
                    return;
                }

                // Check if the user has been down for more than 2 hours.
                let dt_now = chrono::Utc::now().timestamp_millis();
                if dt_now - dt_start > 2 * 60 * 60 * 1000 {
                    evict_user(user, tree, dids, pg_pool).await;
                }
            }
        },
    }
}

// Process a firehose message.
async fn process(
    message: Vec<u8>, tree: &'static BulkSearchTree, dids: &'static RwLock<HashMap<String, Arc<User>>>,
    http_client: reqwest::Client, pg_pool: &'static Pool,
) {
    match rsky_firehose::firehose::read(&message) {
        Ok((_header, body)) => {
            if let SubscribeRepos::Commit(commit) = body {
                for op in commit.ops {
                    if let Some(cid) = op.cid {
                        if !op.path.starts_with("app.bsky.feed.post/") {
                            continue;
                        }

                        let mut car_reader = Cursor::new(&commit.blocks);
                        let _ = rsky_firehose::car::read_header(&mut car_reader).unwrap();
                        let car_blocks = rsky_firehose::car::read_blocks(&mut car_reader).unwrap();
                        let record_reader = Cursor::new(car_blocks.get(&cid).unwrap());
                        match serde_cbor::from_reader(record_reader) {
                            Ok(Lexicon::AppBskyFeedPost(post)) => {
                                // Get the timestamp in seconds.
                                let ts_seconds = chrono::Utc::now().timestamp();

                                // Find the search match users and inform them.
                                let text_lower = post.text.to_lowercase();
                                let search_match_users = tree.find_all_matches(&text_lower).await;
                                let uri = format!("at://{}/{}", commit.repo, op.path);
                                let post_uri_json = serde_json::to_string(&json!({
                                    "uri": uri,
                                    "post": post,
                                })).unwrap();
                                let json_arc = Arc::new(post_uri_json);
                                let mut used_ids = HashSet::new();
                                for user in search_match_users.into_iter() {
                                    let json_clone = json_arc.clone();
                                    let client_cpy = http_client.clone();
                                    used_ids.insert(user.id);
                                    let tree_ref = tree;
                                    let dids_ref = dids;
                                    tokio::spawn(async move {
                                        inform_user(
                                            user, json_clone, ts_seconds, client_cpy, tree_ref, dids_ref, pg_pool
                                        ).await;
                                    });
                                }

                                // Find any DID mentions in the post and then check if we have a user for that DID.
                                for facet in post.facets.as_ref().unwrap_or(&vec![]).into_iter() {
                                    let features_ref = &facet.features;
                                    for feature in features_ref.into_iter() {
                                        if let Features::Mention(mention) = &feature {
                                            let lock = dids.read().await;
                                            let user = lock.get(mention.did.as_str()).cloned();
                                            if let Some(user) = user {
                                                // Check if the user was already informed about this post and if not, inform them.
                                                if !used_ids.contains(&user.id) {
                                                    let json_clone = json_arc.clone();
                                                    let client_cpy = http_client.clone();
                                                    let tree_ref = tree;
                                                    let dids_ref = dids;
                                                    tokio::spawn(async move {
                                                        inform_user(
                                                            user, json_clone, ts_seconds, client_cpy,
                                                            tree_ref, dids_ref, pg_pool
                                                        ).await;
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("Error parsing firehose message: {error:?}");
        }
    }
}

#[tokio::main]
async fn main() {
    // Create the tree.
    let tree = Box::leak(Box::new(BulkSearchTree::new()));

    // Create the DID map.
    let dids = Box::leak(Box::new(RwLock::new(HashMap::new())));

    // Create the Postgres pool.
    let pg_pool = Box::leak(Box::new(init_postgres()));

    // Initialize the data in our local copy.
    init_data(pg_pool, tree, dids).await;

    // Create the HTTP server.
    tokio::spawn(async {
        init_http_server(pg_pool, tree, dids).await;
    });

    // Create the HTTP client.
    let http_client = Box::leak(Box::new(reqwest::Client::new()));

    // Connect to the firehose.
    loop {
        match tokio_tungstenite::connect_async(
            "wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos",
        )
        .await
        {
            Ok((mut socket, _response)) => {
                println!("Connected to the firehose. Brrrrr!");
                while let Some(Ok(Message::Binary(message))) = socket.next().await {
                    let client_cpy = http_client.clone();
                    tokio::spawn(async {
                        process(message, tree, dids, client_cpy, pg_pool).await;
                    });
                }
            }
            Err(error) => {
                eprintln!("Error connecting to the firehose. Waiting to reconnect: {error:?}");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        }
    }
}
