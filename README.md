# Bluehook

Webhooks for Bluesky! This consists of a Rust worker which handles the firehose and a webapp that handles adding users to the database and telling the worker when to grab new data from the database.

Documentation is a work in progress, but [this example here is likely very useful to you!](https://github.com/IAmJSD/bluehook-example/blob/main/app/api/bluesky/route.ts)

## Deployment

If you wish to self-host this, you will want to do the following:

- Create a Postgres database and run `worker/schema.sql` against it. I like Neon for this (disclaimer: I am affiliated with them). Take the connection string and store it somewhere.
- Create a random string and store it somewhere.
- Deploy `worker` to a suitable node. The artifact is available here, and you can use the kubernetes templates to get started. Set `HTTP_KEY` to the random string and `PG_CONNECTION_STRING` to the connection string. Make a HTTPS proxy to the worker service.
- Deploy `web` to a suitable platform. I personally use Vercel. Set `SERVER_HOSTNAME` to the hostname of the server running the worker, `HTTP_KEY` to the random string, and `PG_CONNECTION_STRING` to the connection string.
