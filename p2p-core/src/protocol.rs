//! Protocol primitives for the P2P games platform.
//!
//! This module defines a transport-agnostic message envelope and the
//! payload types needed for global chat, discovery (room announcements),
//! and basic room management. Messages can be serialized via `serde`
//! (e.g., JSON/CBOR) and sent over any transport (iroh gossip, TCP, …).
//!
//! # Design goals
//! - **Stable envelope**: versioned header, generic `body`.
//! - **Extensible**: add new `Kind`/payload variants without breaking old peers.
//! - **Transport-agnostic**: the transport deals with bytes; (de)serialization
//!   happens at the application layer.
//!

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VER: u16 = 1;

/// High-level message category.
///
/// Used by routers/handlers to dispatch messages to the right subsystem.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Kind {
    /// Discovery: announce/list rooms in the global topic.
    Discovery,
    /// Room control: join/ack/members/leave/close inside a room topic.
    Room,
    /// Chat messages (global or room-scoped).
    Chat,
    /// Reserved for game lifecycle and gameplay messages (start/cmd/state/events).
    Game,
}

/// Logical broadcast scope of a message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Scope {
    /// Sent/received on a **global** topic (visible to all peers).
    Global,
    /// Sent/received on a **room** topic (only members of a lobby/game).
    Room,
}

/// Common envelope for **all** messages.
///
/// The envelope carries versioning, addressing and metadata. The generic `T`
/// holds the actual payload (chat/discovery/room/game).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// Protocol version (see [`PROTOCOL_VER`]).
    pub ver: u16,
    /// Message category (for dispatching).
    pub kind: Kind,
    /// Logical scope: global or room.
    pub scope: Scope,
    /// Target room when `scope == Scope::Room`; `None` for global messages.
    pub room_id: Option<String>,
    /// Human-readable peer/node id (e.g., hex/base32 of iroh `NodeId`).
    pub sender_id: String,
    /// Unique id (UUID) for **de-duplication** and tracing.
    pub msg_id: String,
    /// Monotonic timestamp (e.g., unix millis or a Lamport counter).
    /// Used for UI ordering and basic replay protection.
    pub ts: u64,
    /// The actual message payload.
    pub body: T,
}

/// Minimal chat payload.
///
/// Extend later if needed (e.g., attachments, markdown flag, mentions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMsg {
    /// Text content of the chat message.
    pub text: String,
}

/// Discovery messages (global topic).
///
/// Tagged internally with a `"type"` discriminator to keep the wire format simple.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiscoveryBody {
    /// Host announces a newly opened room/lobby.
    AnnounceRoom {
        /// Stable room identifier (e.g., `"lobby-<uuid>"`).
        room_id: String,
        /// Human-readable title shown in the room list.
        title: String,
        /// Peer id of the host (creator of the room).
        host_id: String,
        /// Creation time (unix millis).
        created_at: u64,
    },
    /// Ask peers to respond with the rooms they currently know/host.
    ListRoomsReq,
    /// Batched response containing known rooms (could be multicast on global topic).
    ListRoomsRes {
        /// Summaries suitable for a lobby list UI.
        rooms: Vec<RoomSummary>,
    },
}

/// Compact room metadata for lobby listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSummary {
    /// Room identifier.
    pub room_id: String,
    /// Display title.
    pub title: String,
    /// Host/owner peer id.
    pub host_id: String,
    /// Last time this room was observed/announced (unix millis).
    pub last_seen: u64,
}

/// Room control messages (room topic).
///
/// Also tagged with `"type"` for a straightforward wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoomBody {
    /// Request to join a room. Sent by a peer to the room topic.
    JoinReq {
        /// Target room id (redundant but explicit/self-contained).
        room_id: String,
        /// Desired display name inside the room.
        nickname: String,
    },
    /// Acknowledge a join attempt (accept/reject). Sent by the host.
    JoinAck {
        /// Room id.
        room_id: String,
        /// Whether the join was accepted.
        accept: bool,
        /// Optional reason if rejected.
        reason: Option<String>,
    },
    /// Canonical member list broadcast by the host after changes.
    Members {
        /// Room id.
        room_id: String,
        /// Peer id of the current host (leader).
        host_id: String,
        /// Current members (peer id + nickname).
        members: Vec<Member>,
    },
    /// Voluntary leave notification from a peer.
    Leave {
        /// Room id.
        room_id: String,
    },
    /// Room closure (only host). Peers should unsubscribe and clear local state.
    Close {
        /// Room id.
        room_id: String,
    },
}

/// Member entry used in [`RoomBody::Members`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    /// Peer/node identifier.
    pub peer_id: String,
    /// Display name inside the room.
    pub nickname: String,
}

/// Convenience helper to build an [`Envelope`] with an auto-generated `msg_id`.
///
/// Supply semantic parts; we fill in `ver` and a fresh UUID for `msg_id`.
pub fn make_envelope<T>(
    kind: Kind,
    scope: Scope,
    room_id: Option<String>,
    sender_id: String,
    ts: u64,
    body: T,
) -> Envelope<T> {
    Envelope {
        ver: PROTOCOL_VER,
        kind,
        scope,
        room_id,
        sender_id,
        msg_id: Uuid::new_v4().to_string(),
        ts,
        body,
    }
}
