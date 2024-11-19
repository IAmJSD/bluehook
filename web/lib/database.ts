import { Pool } from "@neondatabase/serverless";

let pool: Pool | null = null;

export function getPool() {
    if (pool !== null) {
        return pool;
    }
    const pgConnectionString = process.env.PG_CONNECTION_STRING;
    if (pgConnectionString === undefined) {
        throw new Error("PG_CONNECTION_STRING is not set");
    }
    pool = new Pool({ connectionString: pgConnectionString });
    return pool;
}
