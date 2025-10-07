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

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ======================================================================
// Constants
// ======================================================================

/// Protocol version for wire compatibility checks.
pub const PROTOCOL_VER: u16 = 1;

/// Human-readable name for the global chat topic (transport maps this string to a topic id).
pub const GLOBAL_CHAT_TOPIC_NAME: &str = "p2p-global-chat";

/// Human-readable name for the nickname registry topic (transport maps this string to a topic id).
pub const NAME_REGISTRY_TOPIC_NAME: &str = "p2p-name-registry";

// ======================================================================
// Core protocol: Envelope, Kind, Scope
// ======================================================================

/// High-level message category used to dispatch to subsystems.
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

// ======================================================================
// Payloads
// ======================================================================

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

/// Nickname claim broadcast on the name-registry topic.
///
/// The network reaches a deterministic decision about the owner by applying
/// [`name_claim_wins`] consistently on all peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameClaim {
    pub nick_lower: String,
    pub nickname: String,
    pub owner_peer_id: String,
    pub since_ts: u64,
}

// ======================================================================
// Transport-agnostic helpers (time, ser/de, builders, rules)
// ======================================================================

/// Current unix timestamp in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Serialize an envelope to JSON bytes (transport sends raw bytes).
pub fn to_json_bytes<T: Serialize>(env: &Envelope<T>) -> Vec<u8> {
    serde_json::to_vec(env).expect("serialize envelope")
}

/// Try to deserialize JSON bytes to an envelope.
///
/// Returns `None` if the payload type does not match or JSON is invalid.
pub fn from_json_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Option<Envelope<T>> {
    serde_json::from_slice(bytes).ok()
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

/// Deterministic winner rule for nickname ownership.
///
/// *Primary key*: smaller `since_ts` wins (earlier claim).
/// *Tie-breaker*: lexicographically smaller `owner_peer_id` wins.
pub fn name_clame_wins(a_owner: &str, a_ts: u64, b_owner: &str, b_ts: u64) -> bool {
    if a_ts != b_ts {
        a_ts < b_ts
    } else {
        a_owner < b_owner
    }
}

/// Build a global chat envelope (ready for serialization and send).
pub fn make_chat_global(sender_id: String, text: impl Into<String>) -> Envelope<ChatMsg> {
    make_envelope(
        Kind::Chat,
        Scope::Global,
        None,
        sender_id,
        now_ms(),
        ChatMsg { text: text.into() },
    )
}

/// Build a room-scoped chat envelope (ready for serialization and send).
pub fn make_chat_room(
    room_id: impl Into<String>,
    sender_id: String,
    text: impl Into<String>,
) -> Envelope<ChatMsg> {
    make_envelope(
        Kind::Chat,
        Scope::Room,
        Some(room_id.into()),
        sender_id,
        now_ms(),
        ChatMsg { text: text.into() },
    )
}

// ======================================================================
// CLI command model (keeps main.rs small; transport-agnostic)
// ======================================================================

/// Top-level CLI parser for the application.
#[derive(Parser, Debug)]
#[command(name = "p2p-games", about = "P2P games over gossip")]
pub struct AppCli {
    #[command(subcommand)]
    pub command: Command,
}

/// High-level commands exposed to the user.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Claim a unique nickname in the P2P network.
    Login {
        /// Desired nickname.
        #[arg(long)]
        name: String,
        /// If set, do not auto-rename on conflict (exit non-zero instead).
        #[arg(long, default_value_t = false)]
        no_auto: bool,
        /// Wait time (ms) to collect registry claims.
        #[arg(long, default_value_t = 1200)]
        wait_ms: u64,
    },
    /// Print your node address (share with peers to enable direct connections).
    Addr,
    /// Global chat (fixed topic).
    Global {
        /// Global subcommand (listen/say).
        #[command(subcommand)]
        sub: GlobalCmd,
    },
    /// Room operations (you can have at most one active room).
    Room {
        /// Room subcommand (open/join/leave/say).
        #[command(subcommand)]
        sub: RoomCmd,
    },
    /// Show local identity / session information.
    Whoami,
}

/// Subcommands for the global chat.
#[derive(Subcommand, Debug)]
pub enum GlobalCmd {
    /// Listen to messages in the global chat.
    Listen,
    /// Send a message to the global chat.
    Say { text: String },
}

/// Subcommands for room handling.
#[derive(Subcommand, Debug)]
pub enum RoomCmd {
    /// Open a room by name (becomes your active room).
    Open { name: String },
    /// Join a room via host node address & topic hex (becomes active room).
    Join {
        /// Host node address string (e.g., from `Addr` or `Open` output).
        #[arg(long)]
        addr: String,
        /// Topic hex (32-byte topic id as hex; e.g., from `Open` output).
        #[arg(long)]
        topic: String,
    },
    /// Leave the currently active room.
    Leave,
    /// Say a line into the currently active room.
    Say { text: String },
    /// List known/open rooms announced on the network.
    List,
}
