//! The REST surfaces described as OpenAPI 3.0 documents: [`document`] covers the
//! admin control plane (served unauthenticated at `GET /admin/openapi.json` on
//! the admin host) and [`player_document`] covers player auth (served at
//! `GET /openapi.json` on the game host).
//!
//! Hand-written rather than derived: the whole surface is a handful of endpoints,
//! and we removed the `schemars` machinery in Stage 12 — an explicit literal is
//! simpler than re-adding a derive framework, and it lives right next to the
//! routes it documents. Keep it in sync with [`crate::admin`] / [`crate::player`]
//! and the `netplay-protocol` response types by hand (tests pin every route).

use serde_json::{json, Value};

/// Build the admin OpenAPI document. Cheap enough to construct per request (the
/// admin API is low-traffic), so we don't cache it.
pub fn document() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Netplay relay — admin API",
            "version": "1.0.0",
            "description":
                "Control plane for the netplay relay. Served on the admin host \
                 (X-Forwarded-Host = NETPLAY_ADMIN_HOST); the gameplay WebSocket is \
                 a separate surface. Log in for a bearer token, then read lobby state."
        },
        "servers": [
            { "url": "https://admin.netplay.oliverj.network" }
        ],
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "description":
                        "A session token from POST /admin/login or POST /admin/tokens."
                }
            },
            "schemas": {
                "LoginRequest": {
                    "type": "object",
                    "required": ["name", "password"],
                    "properties": {
                        "name": { "type": "string" },
                        "password": { "type": "string" }
                    }
                },
                "TokenRequest": {
                    "type": "object",
                    "properties": {
                        "days": {
                            "type": "integer",
                            "format": "int64",
                            "description":
                                "Requested lifetime in days; clamped to [1, 90]. \
                                 Defaults to 30 when omitted.",
                            "minimum": 1,
                            "maximum": 90
                        }
                    }
                },
                "TokenResponse": {
                    "type": "object",
                    "required": ["token", "expires_in_hours"],
                    "properties": {
                        "token": {
                            "type": "string",
                            "description": "Bearer token; send as `Authorization: Bearer <token>`."
                        },
                        "expires_in_hours": { "type": "integer", "format": "int64" }
                    }
                },
                "PlayerInfo": {
                    "type": "object",
                    "required": ["id", "name"],
                    "properties": {
                        "id": { "type": "integer", "format": "int64" },
                        "name": { "type": "string" }
                    }
                },
                "MatchInfo": {
                    "type": "object",
                    "required": ["seat0", "seat1"],
                    "description": "The two paired players; seat0 moves first.",
                    "properties": {
                        "seat0": { "$ref": "#/components/schemas/PlayerInfo" },
                        "seat1": { "$ref": "#/components/schemas/PlayerInfo" }
                    }
                },
                "ServerStats": {
                    "type": "object",
                    "required": ["players_online", "matches_active", "uptime_seconds"],
                    "properties": {
                        "players_online": { "type": "integer", "format": "int32" },
                        "matches_active": { "type": "integer", "format": "int32" },
                        "uptime_seconds": { "type": "integer", "format": "int64" }
                    }
                }
            },
            "responses": {
                "Unauthorized": {
                    "description": "Missing or invalid bearer token."
                },
                "BadRequest": {
                    "description": "Malformed, oversized, or invalid request body (plain text says why)."
                },
                "ServerError": {
                    "description": "Database or lobby failure."
                }
            }
        },
        "paths": {
            "/admin/login": {
                "post": {
                    "summary": "Exchange admin credentials for a short-lived bearer token.",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/LoginRequest" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "A 24-hour session token.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/TokenResponse" }
                                }
                            }
                        },
                        "400": { "$ref": "#/components/responses/BadRequest" },
                        "401": { "description": "Wrong name or password." },
                        "403": { "description": "The account exists but is not an admin." },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/admin/tokens": {
                "post": {
                    "summary": "Trade a valid bearer for a longer-lived (durable) token.",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": false,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/TokenRequest" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "A durable session token (default 30 days).",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/TokenResponse" }
                                }
                            }
                        },
                        "400": { "$ref": "#/components/responses/BadRequest" },
                        "401": { "$ref": "#/components/responses/Unauthorized" },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/admin/players": {
                "get": {
                    "summary": "List the players currently connected to the lobby.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "The connected players.",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/PlayerInfo" }
                                    }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/admin/matches": {
                "get": {
                    "summary": "List the active matches.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "The active matches.",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/MatchInfo" }
                                    }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/admin/stats": {
                "get": {
                    "summary": "A snapshot of relay counters.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "The relay stats.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ServerStats" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/admin/openapi.json": {
                "get": {
                    "summary": "This document.",
                    "responses": {
                        "200": {
                            "description": "The OpenAPI description of the admin API.",
                            "content": { "application/json": {} }
                        }
                    }
                }
            }
        }
    })
}

/// Build the game-host OpenAPI document: player auth (`/login`, `/register`).
/// The gameplay WebSocket itself is out of OpenAPI's scope (it models HTTP);
/// the wire messages live in `netplay-protocol`.
pub fn player_document() -> Value {
    let token_200 = json!({
        "description": "A 24-hour session token; present it in the WebSocket Hello.",
        "content": {
            "application/json": {
                "schema": { "$ref": "#/components/schemas/TokenResponse" }
            }
        }
    });
    let credentials_body = json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": { "$ref": "#/components/schemas/LoginRequest" }
            }
        }
    });
    let admin = document();
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Netplay relay — player auth",
            "version": "1.0.0",
            "description":
                "Player authentication for the netplay relay. Exchange account \
                 credentials for a bearer token, then open the gameplay WebSocket \
                 on this same host with the token in the Hello message."
        },
        "servers": [
            { "url": "https://relay.netplay.oliverj.network" }
        ],
        "components": {
            // Reuse the shared shapes from the admin document so they can't drift.
            "schemas": {
                "LoginRequest": admin["components"]["schemas"]["LoginRequest"],
                "TokenResponse": admin["components"]["schemas"]["TokenResponse"]
            },
            "responses": {
                "BadRequest": admin["components"]["responses"]["BadRequest"],
                "ServerError": admin["components"]["responses"]["ServerError"]
            }
        },
        "paths": {
            "/login": {
                "post": {
                    "summary": "Log in an existing account for a session token.",
                    "requestBody": credentials_body,
                    "responses": {
                        "200": token_200,
                        "400": { "$ref": "#/components/responses/BadRequest" },
                        "401": { "description": "Wrong name or password." },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/register": {
                "post": {
                    "summary": "Create an account (open registration) and get a session token.",
                    "requestBody": credentials_body,
                    "responses": {
                        "200": token_200,
                        "400": {
                            "description":
                                "Empty name, name over 32 characters, password under \
                                 8 characters, or a malformed body (plain text says which)."
                        },
                        "409": { "description": "That name is taken." },
                        "500": { "$ref": "#/components/responses/ServerError" }
                    }
                }
            },
            "/openapi.json": {
                "get": {
                    "summary": "This document.",
                    "responses": {
                        "200": {
                            "description": "The OpenAPI description of player auth.",
                            "content": { "application/json": {} }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_document_is_well_formed_and_covers_every_route() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.0.3");
        let paths = doc["paths"].as_object().expect("paths object");
        for route in [
            "/admin/login",
            "/admin/tokens",
            "/admin/players",
            "/admin/matches",
            "/admin/stats",
            "/admin/openapi.json",
        ] {
            assert!(paths.contains_key(route), "missing path {route}");
        }
        // The read endpoints require the bearer scheme; login does not.
        assert!(doc["paths"]["/admin/stats"]["get"]["security"].is_array());
        assert!(doc["paths"]["/admin/login"]["post"]["security"].is_null());
        // The handlers' full status surface is documented (drift check).
        for (path, codes) in [
            ("/admin/login", vec!["200", "400", "401", "403", "500"]),
            ("/admin/tokens", vec!["200", "400", "401", "500"]),
            ("/admin/stats", vec!["200", "401", "500"]),
        ] {
            let method = if path == "/admin/stats" {
                "get"
            } else {
                "post"
            };
            let responses = doc["paths"][path][method]["responses"]
                .as_object()
                .expect("responses");
            for code in codes {
                assert!(responses.contains_key(code), "{path} missing {code}");
            }
        }
    }

    #[test]
    fn player_document_covers_login_and_register() {
        let doc = player_document();
        assert_eq!(doc["openapi"], "3.0.3");
        for route in ["/login", "/register", "/openapi.json"] {
            assert!(
                doc["paths"].as_object().unwrap().contains_key(route),
                "missing path {route}"
            );
        }
        // 409 (name taken) is register-only; both share the token response shape.
        assert!(doc["paths"]["/register"]["post"]["responses"]["409"].is_object());
        assert!(doc["components"]["schemas"]["TokenResponse"].is_object());
    }
}
