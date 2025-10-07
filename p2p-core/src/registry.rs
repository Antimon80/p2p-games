use anyhow::Result;
use tokio::time::{timeout, Duration};
use std::collections::BTreeMap;

use crate::protocol::{
    Envelope, NameClaim, NAME_REGISTRY_TOPIC_NAME, now_ms, name_claim_wins,
};
use transport_iroh::transport_iroh::GossipTransport;

#[derive(Debug, Default, Clone)]
pub struct NameTable {
    owners: BTreeMap<String, (String, u64, String)>,
}

impl NameTable {
    pub fn apply(&mut self, c: &NameClaim) {
        match self.owners.get(&c.nick_lower) {
            None => {
                self.owners.insert(c.nick_lower.clone(), (c.owner_peer_id.clone(), c.since_ts, c.nickname.clone()));
            }
            Some((owner, since, casing)) => {
                if *owner == c.owner_peer_id {
                    if &c.nickname != casing {
                        self.owners.insert(c.nick_lower.clone(), (owner.clone(), *since, c.nickname.clone()));
                    }
                } else {
                    let win_new = name_claim_wins(&c.owner_peer_id, c.since_ts, owner, *since);
                    let(w_owner, w_ts, w_name) = if win_new {
                        (c.owner_peer_id.clone(), c.since_ts, c.nickname.clone())
                    } else {
                        (owner.clone(), *since, casing.clone())
                    };
                    self.owners.insert(c.nick_lower.clone(), (w_owner, w_ts, w_name));
                }
            }
        }
    }

    pub fn owner(&self, nick_lower: &str) -> Option<&(String, u64, String)> {
        self.owners.get(nick_lower)
    }
}

pub struct NameRegistry<'a> {
        transport: &'a dyn GossipTransport,
    }

    impl<'a> NameRegistry<'a> {
        pub fn new(transport: &'a dyn GossipTransport) -> Self {
            Self {
                transport
            }
        }

        pub async fn claim_unique(&self, desired: &str, my_peer_id: &str, wait_ms: u64) -> Result<(String, bool)> {
            let topic = self.transport.topic_from_name(NAME_REGISTRY_TOPIC_NAME);
            let mut th = self.transport.join_topic(topic).await?;

            let claim = NameClaim {
                nick_lower: desired.to_lowercase(),
                nickname: desired.to_string(),
                owner_peer_id: my_peer_id.to_string(),
                since_ts: now_ms(),
            };

            let env = Envelope {
                ver: crate::protocol::PROTOCOL_VER,
                kind: crate::protocol::Kind::Room,
                scope: crate::protocol::Scope::Global,
                room_id: None,
                sender_id: my_peer_id.to_string(),
                msg_id: uuid::Uuid::new_v4().to_string(),
                ts: claim.since_ts,
                body: claim.clone(),
            };

            let bytes = serde_json::to_vec(&env)?;
            th.publish(&bytes).await?;

            let mut table = NameTable::default();
            table.apply(&claim);

            let _ = timeout(Duration::from_millis(wait_ms), async {
                loop {
                    match th.next().await {
                        Ok(b) => {
                            if let Ok(env) = serde_json::from_slice::<Envelope<NameClaim>> (&b) {
                                table.apply(&env.body);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }).await;

            if let Some((owner, _, name)) = table.owner(&desired.to_lowercase()) {
                if owner == my_peer_id {
                    return Ok((name.clone(), true))
                }
            }

            let suffix = &my_peer_id[..6.min(my_peer_id.len())];
            Ok((format!("{}-{}", desired, suffix), false))
        }
    }