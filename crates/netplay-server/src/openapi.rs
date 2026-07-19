//! The admin REST API described as an OpenAPI 3.0 document, served (unauthenticated)
//! at `GET /admin/openapi.json` so a client can discover the surface.
//!
//! Hand-written rather than derived: the API is five endpoints, and we removed the
//! `schemars` machinery in Stage 12 — an explicit literal is simpler than re-adding
//! a derive framework, and it lives right next to the routes it documents. Keep it in
//! sync with [`crate::admin`] and the `netplay-protocol` response types by hand.

use serde_json::{json, Value};

/// Build the OpenAPI document. Cheap enough to construct per request (the admin
/// API is low-traffic), so we don't cache it.
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
                        "401": { "description": "Wrong name or password." },
                        "403": { "description": "The account exists but is not an admin." }
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
                        "401": { "$ref": "#/components/responses/Unauthorized" }
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
                        "401": { "$ref": "#/components/responses/Unauthorized" }
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
                        "401": { "$ref": "#/components/responses/Unauthorized" }
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
                        "401": { "$ref": "#/components/responses/Unauthorized" }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_well_formed_and_covers_every_route() {
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
    }
}
