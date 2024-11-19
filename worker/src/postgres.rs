use std::{collections::HashMap, sync::Arc};
use deadpool_postgres::{Config, GenericClient, ManagerConfig, Object, Pool, RecyclingMethod, Runtime};
use tokio::sync::RwLock;
use crate::bulk_search_tree::{BulkSearchTree, User};

// Setup a connection pool to the Postgres database.
pub fn init_postgres() -> Pool {
    // Find the PG_CONNECTION_STRING environment variable.
    let pg_connection_string = std::env::var("PG_CONNECTION_STRING").expect("PG_CONNECTION_STRING must be set");

    // Setup a SSL pool using the certificate authorities on the system.
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect(),
    };
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    // Create the pool.
    let mut deadpool_cfg = Config::new();
    deadpool_cfg.url = Some(pg_connection_string);
    deadpool_cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
    deadpool_cfg.create_pool(Some(Runtime::Tokio1), tls).unwrap()
}

// Delete a user from the pool by their private key.
pub async fn delete_user(pool: &Pool, private_key: &str) {
    let conn = pool.get().await.unwrap();
    conn.execute(
        "DELETE FROM users WHERE private_key = $1", &[&private_key]
    ).await.unwrap();
}

// Internal function to load in a specific user.
async fn load_user(
    conn: &Object, mut user: User, tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>,
) {
    let hex_s = hex::encode(&user.private_key);
    let rows = conn.query(
        "SELECT phrase FROM phrases WHERE private_key = $1", &[&hex_s]
    ).await.unwrap();
    let phrases: Vec<String> = rows.iter().map(|row| row.get::<_,String>(0)).collect();
    user.phrases = phrases;
    let user_arc = Arc::new(user);
    if let Some(did) = user_arc.did.clone() {
        dids.write().await.insert(did, user_arc.clone());
    }
    for phrase in user_arc.phrases.iter() {
        tree.add_item(phrase.as_str(), user_arc.clone()).await;
    }
}

// Initialize the data in our local copy.
pub async fn init_data(pool: &Pool, tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>) {
    let conn = pool.get().await.unwrap();
    let rows = conn.query(
        "SELECT did, endpoint, private_key FROM users", &[]
    ).await.unwrap();
    for row in rows {
        let did: Option<String> = row.get(0);
        let endpoint: String = row.get(1);
        let private_key: String = row.get(2);
        let user = User::new(did, endpoint, private_key).unwrap();
        load_user(&conn, user, tree, dids).await;
    }
}

// Initialize a new user by their private key.
pub async fn init_user(
    pool: &Pool, tree: &BulkSearchTree, dids: &RwLock<HashMap<String, Arc<User>>>,
    private_key: &str,
) {
    let conn = pool.get().await.unwrap();
    let row = match conn.query_one(
        "SELECT did, endpoint FROM users WHERE private_key = $1", &[&private_key]
    ).await {
        Ok(row) => row,
        Err(e) => {
            eprintln!("Error fetching user: {}", e);
            return;
        }
    };
    let did: Option<String> = row.get(0);
    let endpoint: String = row.get(1);
    let user = User::new(did, endpoint, private_key.to_string()).unwrap();
    load_user(&conn, user, tree, dids).await;
}
