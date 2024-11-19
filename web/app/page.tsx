import Form from "./form";
import nacl from "tweetnacl";
import { getPool } from "@/lib/database";

export const runtime = "edge";

function u8ToHex(u8: Uint8Array) {
    return Array.from(u8)
        .map((x) => x.toString(16).padStart(2, "0"))
        .join("");
}

function generateEd25519KeyPair() {
    const keyPair = nacl.sign.keyPair();
    return {
        publicKey: u8ToHex(keyPair.publicKey),
        privateKey: u8ToHex(keyPair.secretKey.slice(0, 32)),
    };
}

export default function Home() {
    async function handler(phrases: string[], did: string, url: string) {
        "use server";

        if (
            !Array.isArray(phrases) ||
            phrases.some(
                (phrase) => typeof phrase !== "string" || phrase.length < 5,
            ) ||
            typeof url !== "string" ||
            url.length === 0 ||
            typeof did !== "string"
        ) {
            throw new Error("Invalid input");
        }
        new URL(url);

        const didOrNull = did.length === 0 ? null : did;
        if (didOrNull === null && phrases.length === 0) {
            throw new Error("Either phrases or DID must be provided");
        }

        const { publicKey, privateKey } = generateEd25519KeyPair();
        const pool = getPool();

        const conn = await pool.connect();
        await conn.query(
            "INSERT INTO users (private_key, did, endpoint) VALUES ($1, $2, $3)",
            [privateKey, didOrNull, url],
        );

        const phraseSet = new Set(phrases);
        for (const phrase of phraseSet) {
            await conn.query(
                "INSERT INTO phrases (private_key, phrase) VALUES ($1, $2)",
                [privateKey, phrase],
            );
        }

        const serverHostname = process.env.SERVER_HOSTNAME;
        if (serverHostname === undefined) {
            throw new Error("SERVER_HOSTNAME is not set");
        }
        const httpKey = process.env.HTTP_KEY;
        if (httpKey === undefined) {
            throw new Error("HTTP_KEY is not set");
        }
        const res = await fetch(`https://${serverHostname}/${publicKey}`, {
            method: "POST",
            headers: {
                Authorization: httpKey,
            },
        });
        if (!res.ok) {
            throw new Error("Failed to create user");
        }

        return publicKey;
    }

    return <Form submit={handler} />;
}
