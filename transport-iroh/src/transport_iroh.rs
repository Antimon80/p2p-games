use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use iroh::{protocol::Router, Endpoint, NodeAddr, PublicKey, Watcher};
use iroh_gossip::{
    api::{Event, GossipTopic, Message},
    net::Gossip,
    proto::TopicId,
    ALPN,
};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;

#[async_trait]
pub trait TopicHandle: Send + Sync {
    async fn publish(&self, bytes: &[u8]) -> Result<()>;
    async fn next(&mut self) -> Result<Vec<u8>>;
}

#[async_trait]
pub trait GossipTransport: Send + Sync {
    fn node_addr(&self) -> &NodeAddr;
    async fn connect(&self, peer: &NodeAddr) -> Result<()>;
    async fn join_topic(&self, topic: TopicId) -> Result<Box<dyn TopicHandle>>;
    fn topic_from_name(&self, name: &str) -> TopicId;
    fn topic_from_hex(&self, hex: &str) -> Result<TopicId>;
    fn topic_to_hex(&self, topic: &TopicId) -> String;
    fn parse_node_id_addr(&self, s: &str) -> Result<NodeAddr>;
}

pub struct IrohTransport {
    endpoint: Endpoint,
    gossip: Gossip,
    _router: Router,
    addr: NodeAddr,
}

impl IrohTransport {
    pub async fn new() -> Result<Self> {
        let endpoint = Endpoint::builder().discovery_n0().bind().await?;
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, gossip.clone())
            .spawn();
        let addr = endpoint.node_addr().initialized().await;
        Ok(Self {
            endpoint,
            gossip,
            _router: router,
            addr,
        })
    }
}

#[async_trait]
impl GossipTransport for IrohTransport {
    fn node_addr(&self) -> &NodeAddr {
        &self.addr
    }

    async fn connect(&self, peer: &NodeAddr) -> Result<()> {
        let _conn = self.endpoint.connect(peer.clone(), ALPN).await?;
        Ok(())
    }

    async fn join_topic(&self, topic: TopicId) -> Result<Box<dyn TopicHandle>> {
        let topic = self.gossip.subscribe(topic, vec![]).await?;
        Ok(Box::new(IrohTopic {
            topic: Arc::new(Mutex::new(topic)),
        }))
    }

    fn topic_from_name(&self, name: &str) -> TopicId {
        let h = blake3::hash(name.as_bytes());
        TopicId::from_bytes(*h.as_bytes())
    }

    fn topic_from_hex(&self, hex: &str) -> Result<TopicId> {
        let bytes = hex::decode(hex)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("topic hex must decode to 32 bytes"))?;
        Ok(TopicId::from_bytes(arr))
    }

    fn topic_to_hex(&self, topic: &TopicId) -> String {
        hex::encode(topic.as_bytes())
    }

    fn parse_node_id_addr(&self, s: &str) -> Result<NodeAddr> {
        let pk = PublicKey::from_str(s)?;
        Ok(NodeAddr::from(pk))
    }
}

struct IrohTopic {
    topic: Arc<Mutex<GossipTopic>>,
}

#[async_trait]
impl TopicHandle for IrohTopic {
    async fn publish(&self, bytes: &[u8]) -> Result<()> {
        let mut topic = self.topic.lock().await;
        topic.broadcast(Bytes::copy_from_slice(bytes)).await?;
        Ok(())
    }

    async fn next(&mut self) -> Result<Vec<u8>> {
        let mut topic = self.topic.lock().await;
        while let Some(ev) = topic.next().await {
            match ev? {
                Event::Received(Message { content, .. }) => return Ok(content.to_vec()),
                _ => continue,
            }
        }
        Err(anyhow!("gossip topic closed"))
    }
}
