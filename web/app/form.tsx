"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AlertCircle, Plus, Trash2 } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

export default function Component({
    submit,
}: {
    submit: (phrases: string[], did: string, url: string) => Promise<string>;
}) {
    const [phrases, setPhrases] = useState<string[]>([]);
    const [newPhrase, setNewPhrase] = useState("");
    const [did, setDid] = useState("");
    const [url, setUrl] = useState("");
    const [publicKey, setPublicKey] = useState("");
    const [error, setError] = useState("");

    const addPhrase = () => {
        if (newPhrase.length >= 5) {
            setPhrases([...phrases, newPhrase.toLowerCase()]);
            setNewPhrase("");
        }
    };

    const removePhrase = (index: number) => {
        setPhrases(phrases.filter((_, i) => i !== index));
    };

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setError("");
        setPublicKey("");

        if (!url) {
            setError("URL is required");
            return;
        }

        if (phrases.length === 0 && !did) {
            setError("Either phrases or DID must be provided");
            return;
        }

        try {
            new URL(url);
        } catch {
            setError("Invalid URL");
            return;
        }

        try {
            const result = await submit(phrases, did, url);
            setPublicKey(result);
        } catch (err) {
            setError("An error occurred during submission");
        }
    };

    return (
        <div className="min-h-screen bg-gray-100 py-12 px-4 sm:px-6 lg:px-8">
            <div className="max-w-3xl mx-auto">
                <h1 className="text-3xl font-bold text-center mb-2">
                    Bluehook
                </h1>
                <p className="text-center mb-8 text-gray-600">
                    The best way to do webhooks on Bluesky. To use this, you
                    will need an active API endpoint setup <a href="https://github.com/IAmJSD/bluehook-example/blob/main/app/api/bluesky/route.ts">
                        <span className="underline font-bold">like this.</span>
                    </a>{" "}To stop this,
                    either respond with a 429/403 (or any other error code for over 2 hours).
                </p>

                <div className="bg-white shadow-md rounded-lg p-6">
                    <form onSubmit={handleSubmit} className="space-y-6">
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                            <div className="space-y-4">
                                <Label htmlFor="newPhrase">
                                    Add Phrase (5+ characters)
                                </Label>
                                <div className="flex space-x-2">
                                    <Input
                                        id="newPhrase"
                                        value={newPhrase}
                                        onChange={(e) =>
                                            setNewPhrase(e.target.value)
                                        }
                                        placeholder="Enter a phrase"
                                    />
                                    <Button
                                        type="button"
                                        onClick={addPhrase}
                                        disabled={newPhrase.length < 5}
                                    >
                                        <Plus className="h-4 w-4" />
                                    </Button>
                                </div>
                                <div className="space-y-2">
                                    {phrases.map((phrase, index) => (
                                        <div
                                            key={index}
                                            className="flex justify-between items-center bg-gray-100 p-2 rounded"
                                        >
                                            <span>{phrase}</span>
                                            <Button
                                                variant="ghost"
                                                size="sm"
                                                onClick={() =>
                                                    removePhrase(index)
                                                }
                                            >
                                                <Trash2 className="h-4 w-4" />
                                            </Button>
                                        </div>
                                    ))}
                                </div>
                            </div>

                            <div className="space-y-4">
                                <div>
                                    <Label htmlFor="did">DID</Label>
                                    <Input
                                        id="did"
                                        value={did}
                                        onChange={(e) => setDid(e.target.value)}
                                        placeholder="Enter DID"
                                    />
                                </div>
                                <div>
                                    <Label htmlFor="url">URL (required, your application endpoint, NOT a Discord webhook/Bluesky URL)</Label>
                                    <Input
                                        id="url"
                                        value={url}
                                        onChange={(e) => setUrl(e.target.value)}
                                        placeholder="Enter URL"
                                        required
                                    />
                                </div>
                            </div>
                        </div>

                        <Button type="submit" className="w-full">
                            Submit
                        </Button>
                    </form>

                    {error && (
                        <Alert variant="destructive" className="mt-4">
                            <AlertCircle className="h-4 w-4" />
                            <AlertTitle>Error</AlertTitle>
                            <AlertDescription>{error}</AlertDescription>
                        </Alert>
                    )}

                    {publicKey && (
                        <Alert className="mt-4">
                            <AlertTitle>Success</AlertTitle>
                            <AlertDescription>
                                This is your public key. All messages should be
                                verified with this: {publicKey}
                            </AlertDescription>
                        </Alert>
                    )}
                </div>

                <footer className="mt-8 text-center text-sm text-gray-500">
                    Created by <a href="https://astrid.place">astrid.place</a>{" "}
                    with a lot of soda, Rust, and TypeScript! Thanks to the Blacksky
                    group for all of the amazing libraries we are using!
                </footer>
            </div>
        </div>
    );
}
