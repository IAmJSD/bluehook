CREATE TABLE users (
    private_key TEXT PRIMARY KEY,
    did TEXT,
    endpoint TEXT NOT NULL
);

CREATE TABLE phrases (
    private_key TEXT NOT NULL REFERENCES users(private_key) ON DELETE CASCADE,
    phrase TEXT NOT NULL,
    PRIMARY KEY (private_key, phrase)
);
