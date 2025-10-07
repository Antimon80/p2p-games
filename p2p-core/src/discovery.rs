use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::time::{Duration, timeout};

use crate::protocol::{DiscoveryBody, Envelope, PROTOCOL_VER, RoomSummary, now_ms};
use transport_iroh::transport_iroh::GossipTransport;

const ROOM_REGISTRY_TOPIC_NAME: &str = "p2p-room-registry";
const DISCOVERY_TOPIC_NAME: &str = "p2p-discovery";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomClaim {
    pub name_lower: String,
    pub name: String,
    pub owner_peer_id: String,
    pub since_ts: u64,
    pub room_id: String,
}

fn room_claim_wins(a_owner: &str, a_ts: u64, b_owner: &str, b_ts: u64) -> bool {
    if a_ts != b_ts {
        a_ts < b_ts
    } else {
        a_owner < b_owner
    }
}

#[derive(Default)]
pub struct RoomTable {
    names: BTreeMap<String, (String, u64, String, String)>,
}
impl RoomTable {
    pub fn apply_claim(&mut self, c: &RoomClaim) {
        match self.names.get(&c.name_lower) {
            None => {
                self.names.insert(
                    c.name_lower.clone(),
                    (
                        c.owner_peer_id.clone(),
                        c.since_ts,
                        c.name.clone(),
                        c.room_id.clone(),
                    ),
                );
            }
            Some((owner, since, disp, rid)) => {
                if *owner == c.owner_peer_id {
                    if (&c.name != disp) || (&c.room_id != rid) {
                        self.names.insert(
                            c.name_lower.clone(),
                            (owner.clone(), *since, c.name.clone(), c.room_id.clone()),
                        );
                    }
                } else {
                    let win_new = room_claim_wins(&c.owner_peer_id, c.since_ts, owner, *since);
                    let (w_owner, w_ts, w_name, w_rid) = if win_new {
                        (
                            c.owner_peer_id.clone(),
                            c.since_ts,
                            c.name.clone(),
                            c.room_id.clone(),
                        )
                    } else {
                        (owner.clone(), *since, disp.clone(), rid.clone())
                    };
                    self.names
                        .insert(c.name_lower.clone(), (w_owner, w_ts, w_name, w_rid));
                }
            }
        }
    }
    pub fn owner_of(&self, name_lower: &str) -> Option<&(String, u64, String, String)> {
        self.names.get(name_lower)
    }
    pub fn all(&self) -> Vec<(String, String, String)> {
        self.names
            .values()
            .map(|(owner, _ts, name, rid)| (name.clone(), rid.clone(), owner.clone()))
            .collect()
    }
}

pub struct Discovery<'a> {
    transport: &'a dyn GossipTransport,
}

impl<'a> Discovery<'a> {
    pub fn new(transport: &'a dyn GossipTransport) -> Self {
        Self { transport }
    }

    pub async fn claim_room_name(
        &self,
        desired_name: &str,
        my_peer_id: &str,
        wait_ms: u64,
    ) -> Result<(String, bool)> {
        let reg_topic = self.transport.topic_from_name(ROOM_REGISTRY_TOPIC_NAME);
        let mut th = self.transport.join_topic(reg_topic).await?;

        let room_id = format!("lobby-{}", uuid::Uuid::new_v4());
        let claim = RoomClaim {
            name_lower: desired_name.to_lowercase(),
            name: desired_name.to_string(),
            owner_peer_id: my_peer_id.to_string(),
            since_ts: now_ms(),
            room_id: room_id.clone(),
        };

        let env = Envelope {
            ver: PROTOCOL_VER,
            kind: crate::protocol::Kind::Discovery,
            scope: crate::protocol::Scope::Global,
            room_id: None,
            sender_id: my_peer_id.to_string(),
            msg_id: uuid::Uuid::new_v4().to_string(),
            ts: claim.since_ts,
            body: claim.clone(),
        };
        th.publish(&serde_json::to_vec(&env)?).await?;

        let mut table = RoomTable::default();
        table.apply_claim(&claim);

        let _ = timeout(Duration::from_millis(wait_ms), async {
            loop {
                match th.next().await {
                    Ok(b) => {
                        if let Ok(env) = serde_json::from_slice::<Envelope<RoomClaim>>(&b) {
                            table.apply_claim(&env.body);
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .await;

        if let Some((owner, _ts, _name, rid)) = table.owner_of(&desired_name.to_lowercase()) {
            if owner == my_peer_id {
                return Ok((rid.clone(), true));
            }
        }
        Ok((room_id, false))
    }

    pub async fn announce_room(&self, room_id: &str, title: &str, host_id: &str) -> Result<()> {
        let topic = self.transport.topic_from_name(DISCOVERY_TOPIC_NAME);
        let th = self.transport.join_topic(topic).await?;
        let body = DiscoveryBody::AnnounceRoom {
            room_id: room_id.to_string(),
            title: title.to_string(),
            host_id: host_id.to_string(),
            created_at: now_ms(),
        };
        let env = Envelope {
            ver: PROTOCOL_VER,
            kind: crate::protocol::Kind::Discovery,
            scope: crate::protocol::Scope::Global,
            room_id: None,
            sender_id: host_id.to_string(),
            msg_id: uuid::Uuid::new_v4().to_string(),
            ts: now_ms(),
            body,
        };
        th.publish(&serde_json::to_vec(&env)?).await?;
        Ok(())
    }

    pub async fn list_rooms(&self, wait_ms: u64) -> Result<Vec<RoomSummary>> {
        let topic = self.transport.topic_from_name(DISCOVERY_TOPIC_NAME);
        let mut th = self.transport.join_topic(topic).await?;

        let req = DiscoveryBody::ListRoomsReq;
        let env = Envelope {
            ver: PROTOCOL_VER,
            kind: crate::protocol::Kind::Discovery,
            scope: crate::protocol::Scope::Global,
            room_id: None,
            sender_id: "client".into(),
            msg_id: uuid::Uuid::new_v4().to_string(),
            ts: now_ms(),
            body: req,
        };
        th.publish(&serde_json::to_vec(&env)?).await?;

        let mut out: Vec<RoomSummary> = Vec::new();
        let _ = timeout(Duration::from_millis(wait_ms), async {
            loop {
                match th.next().await {
                    Ok(b) => {
                        if let Ok(env) = serde_json::from_slice::<Envelope<DiscoveryBody>>(&b) {
                            if let DiscoveryBody::ListRoomsRes { rooms } = env.body {
                                out.extend(rooms);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .await;

        Ok(out)
    }

    pub async fn serve_discovery(
        self,
        known_rooms: impl Fn() -> Vec<RoomSummary> + Send + Sync + 'static,
    ) -> Result<()> {
        let topic = self.transport.topic_from_name(DISCOVERY_TOPIC_NAME);
        let mut th = self.transport.join_topic(topic).await?;

        loop {
            let b = th.next().await?;
            if let Ok(env) = serde_json::from_slice::<Envelope<DiscoveryBody>>(&b) {
                match env.body {
                    DiscoveryBody::ListRoomsReq => {
                        let rooms = known_rooms();
                        let res = DiscoveryBody::ListRoomsRes { rooms };
                        let out = Envelope {
                            ver: PROTOCOL_VER,
                            kind: crate::protocol::Kind::Discovery,
                            scope: crate::protocol::Scope::Global,
                            room_id: None,
                            sender_id: "server".into(),
                            msg_id: uuid::Uuid::new_v4().to_string(),
                            ts: now_ms(),
                            body: res,
                        };
                        th.publish(&serde_json::to_vec(&out)?).await?;
                    }
                    DiscoveryBody::AnnounceRoom { .. } => {}
                    DiscoveryBody::ListRoomsRes { .. } => {}
                }
            }
        }
    }
}
