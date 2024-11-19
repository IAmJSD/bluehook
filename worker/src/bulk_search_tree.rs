use std::{collections::HashSet, sync::{atomic::{AtomicU64, AtomicI64, Ordering}, Arc}};
use hex::FromHexError;
use tokio::sync::RwLock;

// Defines a global ID counter for users.
static USER_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct User {
    // Internally used to manage the tree users fast. Nothing to do with bsky.
    pub id: u64,

    pub did: Option<String>,
    pub phrases: Vec<String>,
    pub endpoint: String,
    pub private_key: Vec<u8>,
    pub user_downtime_started: AtomicI64,
}

impl User {
    pub fn new(
        did: Option<String>, endpoint: String, private_key: String,
    ) -> Result<Self, FromHexError> {
        let private_key = hex::decode(private_key)?;
        Ok(Self {
            id: USER_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            did, phrases: vec![], endpoint, private_key, user_downtime_started: AtomicI64::new(0),
        })
    }
}

#[derive(Default)]
struct BulkSearchBranch {
    // A mapping of path chunks to the next branch. This is a option to allow for splits,
    // but will always be Some.
    mapping: Vec<Option<(Vec<u8>, BulkSearchBranch)>>,

    // A list of users in this branch.
    users: Vec<Arc<User>>,
}

// Recurse through each branch that is relevant to the remaining path. Adds any users from it to the result.
fn walk_branch(
    mut branch: &BulkSearchBranch, mut remaining_path: &[u8], consumed_users: &mut HashSet<u64>,
    users: &mut Vec<Arc<User>>,
) {
'outer:
    loop {
        // Add any users in this branch to the result.
        users.extend(branch.users.iter().filter(|user| {
            let was_uniq = consumed_users.insert(user.id);
            was_uniq
        }).cloned());

        // If we have no more path left then we are done.
        if remaining_path.is_empty() {
            return;
        }

        // Go through each tree node in the current branch.
        for node_opt in branch.mapping.iter() {
            // This will never be None.
            let node = node_opt.as_ref().unwrap();

            // Check if the remaining path starts with this chunk.
            if remaining_path.starts_with(&node.0) {
                // Take the length of the path chunk and remove it from the remaining path.
                remaining_path = &remaining_path[node.0.len()..];

                // Recurse into the next branch.
                branch = &node.1;
                continue 'outer;
            }
        }

        // If we get here then we have no more branches to recurse into.
        break;
    }
}

// Splits a node by creating a new branch and adding the user to the junction.
fn split_node(
    node_opt: &mut Option<(Vec<u8>, BulkSearchBranch)>, split_at: usize, user: Arc<User>,
) {
    let (path, branch) = node_opt.take().unwrap();

    let junction_branch = BulkSearchBranch {
        mapping: vec![
            // Everything after the split point and the old branch.
            Some((path[split_at..].to_vec(), branch)),
        ],
        users: vec![user],
    };

    // Replace the node with the junction branch.
    node_opt.replace((path[..split_at].to_vec(), junction_branch));
}

// Writes to a branch by recursing through and then splitting if needed. Returns true if the user was added.
fn write_branch(mut branch: &mut BulkSearchBranch, mut remaining_path: &[u8], user: Arc<User>) -> bool {
'outer:
    loop {
        // SAFETY: In this context, a unsafe reference copy is safe and based even though it violates Rust's safety guarantees.
        // This is because we will never re-mutably borrow the branch after this point and we know it will stay alive.
        let unsafe_ref = unsafe { &mut *(&mut *branch as *mut BulkSearchBranch) };

        // If we have no more path left then we are done.
        if remaining_path.is_empty() {
            let unique = unsafe_ref.users.iter().find(|u| u.id == user.id).is_none();
            if unique {
                unsafe_ref.users.push(user);
            }
            return unique;
        }

        for node_opt in unsafe_ref.mapping.iter_mut() {
            // SAFETY: This copy is ONLY used in the context of a node split.
            let node_opt_2 = unsafe { &mut *(&mut *node_opt as *mut _) };

            // This will never be None.
            let node = node_opt.as_mut().unwrap();

            // Find out if this chunk is smaller than the remaining path.
            let smaller = node.0.len() <= remaining_path.len();

            if smaller {
                // If we start with this chunk then we should recurse into the next branch.
                if remaining_path.starts_with(&node.0) {
                    remaining_path = &remaining_path[node.0.len()..];
                    branch = &mut node.1;
                    continue 'outer;
                }
            } else if node.0.starts_with(remaining_path) {
                // We need to split this node.
                split_node(node_opt_2, remaining_path.len(), user);
                return true;
            }
        }

        // If no other node matched then we need to create a new node.
        let new_node = BulkSearchBranch {
            mapping: vec![],
            users: vec![user.clone()],
        };
        branch.mapping.push(Some((remaining_path.to_vec(), new_node)));
        return true;
    }
}

// Find a mutable branch that matches EXACTLY the remaining path.
fn find_mut_branch<'a>(mut branch: &'a mut BulkSearchBranch, mut remaining_path: &[u8]) -> Option<&'a mut BulkSearchBranch> {
'outer:
    loop {
        // If we have no more path left then we are done.
        if remaining_path.is_empty() {
            return Some(branch);
        }

        // Go through each tree node in the current branch.
        for node_opt in branch.mapping.iter_mut() {
            // This will never be None.
            let node = node_opt.as_mut().unwrap();

            // Check if the remaining path starts with this chunk.
            if remaining_path.starts_with(&node.0) {
                // Take the length of the path chunk and remove it from the remaining path.
                remaining_path = &remaining_path[node.0.len()..];

                // Recurse into the next branch.
                branch = &mut node.1;
                continue 'outer;
            }
        }

        // If we get here then we have no more branches to recurse into. Return None.
        return None;
    }
}

pub struct BulkSearchTree {
    first_byte: RwLock<Vec<BulkSearchBranch>>,
}

impl BulkSearchTree {
    pub fn new() -> Self {
        // Create the first byte branches.
        let vec_items = (0..=u8::MAX).map(|_| BulkSearchBranch::default()).collect();
        let first_byte = RwLock::new(vec_items);

        Self { first_byte }
    }

    // Finds all users that match witin the given text.
    pub async fn find_all_matches(&self, text: &str) -> Vec<Arc<User>> {
        // Turn it into bytes. We think like a robot.
        let text = text.as_bytes();

        // Read the first byte branches.
        let first_byte_branches = self.first_byte.read().await;

        // Defines all the users we have found so far and a set so we can efficiently check if we already have them.
        let mut users = Vec::new();
        let mut consumed_users = HashSet::new();

        // Iterate over each byte in the text and make a cursor for each iteration.
        for (i, &byte) in text.iter().enumerate() {
            let cursor_after = &text[i + 1..];

            // SAFETY: We can avoid a bounds check here because we know all bytes are initialized.
            let branch = unsafe { first_byte_branches.get_unchecked(byte as usize) };

            // Walk the branch.
            walk_branch(branch, &cursor_after, &mut consumed_users, &mut users);
        }

        // Return the users we found.
        users
    }

    // Adds a user to a tree branch. Return false if the text is blank or the user is already in the tree.
    pub async fn add_item(&self, subtext: &str, user: Arc<User>) -> bool {
        // If the text is blank then we can't add the user.
        if subtext.is_empty() {
            return false;
        }

        // Turn the subtext into bytes.
        let subtext = subtext.as_bytes();

        // Write lock the first byte branches.
        let mut first_byte_branches = self.first_byte.write().await;

        // SAFETY: We can avoid a bounds check here because we know all bytes are initialized.
        let branch = unsafe { first_byte_branches.get_unchecked_mut(subtext[0] as usize) };

        // Get the rest of the path and then write to the branch.
        let rest_path = &subtext[1..];
        write_branch(branch, rest_path, user)
    }

    // Removes a user from the tree. Returns false if the user is not in the tree.
    pub async fn remove_item(&self, subtext: &str, user: Arc<User>) -> bool {
        // Turn the subtext into bytes.
        let subtext = subtext.as_bytes();

        // Bail if the text is blank.
        if subtext.is_empty() {
            return false;
        }

        // Write lock the first byte branches.
        let mut first_byte_branches = self.first_byte.write().await;

        // SAFETY: We can avoid a bounds check here because we know all bytes are initialized.
        let branch = unsafe { first_byte_branches.get_unchecked_mut(subtext[0] as usize) };

        // Get the rest of the path and then delete the user from the branch.
        let rest_path = &subtext[1..];
        let branch = find_mut_branch(branch, rest_path);
        if let Some(branch) = branch {
            branch.users.retain(|u| u.id != user.id);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_user(did: &str, endpoint: &str) -> Arc<User> {
        Arc::new(User::new(
            Some(did.to_string()),
            endpoint.to_string(),
            "aa".to_string(),
        ).unwrap())
    }

    #[tokio::test]
    async fn test_add_and_find_user() {
        let tree = BulkSearchTree::new();
        let user1 = create_user("did:example:123", "http://example.com");
        tree.add_item("hello", user1.clone()).await;
        tree.add_item("world", user1.clone()).await;
        let user2 = create_user("did:example:456", "http://example.com");
        tree.add_item("ab", user2.clone()).await;

        let matches = tree.find_all_matches("hello world").await;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, user1.id);
    }

    #[tokio::test]
    async fn test_multiple_finds() {
        let tree = BulkSearchTree::new();
        let user1 = create_user("did:example:123", "http://example.com");
        tree.add_item("hello", user1.clone()).await;
        tree.add_item("world", user1.clone()).await;
        let user2 = create_user("did:example:456", "http://example.com");
        tree.add_item("hello", user2.clone()).await;

        let matches = tree.find_all_matches("hello world").await;
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|u| u.id == user1.id));
        assert!(matches.iter().any(|u| u.id == user2.id));
    }

    #[tokio::test]
    async fn test_intersecting_phrases() {
        let tree = BulkSearchTree::new();
        let user1 = create_user("did:example:123", "http://example.com");
        tree.add_item("hello", user1.clone()).await;
        tree.add_item("world", user1.clone()).await;
        let user2 = create_user("did:example:456", "http://example.com");
        tree.add_item("or", user2.clone()).await;

        let matches = tree.find_all_matches("hello world").await;
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|u| u.id == user1.id));
        assert!(matches.iter().any(|u| u.id == user2.id));
    }

    #[tokio::test]
    async fn test_remove_user() {
        let tree = BulkSearchTree::new();
        let user = create_user("did:example:123", "http://example.com");

        tree.add_item("hello", user.clone()).await;
        assert!(tree.remove_item("hello", user.clone()).await);

        let matches = tree.find_all_matches("hello").await;
        assert!(matches.is_empty());
    }
}
